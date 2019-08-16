#![feature(try_trait, result_map_or_else)]
use std::ops::Try;

use std::net::UdpSocket;
use std::sync::mpsc;

extern crate log;
use log::{debug, info, warn, error};
extern crate simple_logger;

extern crate jack;
use jack::{Client, Port, MidiIn, MidiOut, RawMidi, ProcessHandler, ProcessScope};

#[macro_use]
extern crate clap;
use clap::{App, AppSettings, Arg, ArgGroup};

extern crate rosc;
use rosc::{OscPacket, OscMessage, OscMidiMessage, OscType, encoder, decoder};

const DEFAULT_SEND_ADDRESS: &str = "localhost:8953";
const DEFAULT_RECEIVE_ADDRESS: &str = "localhost:8953";
const DEFAULT_BIND_ADDRESS: &str = "localhost:0"; // "0.0.0.0:0" does not work

fn main() {
	simple_logger::init().expect("Could not start logger");
	
	jack::set_info_callback(|info| {debug!(target: "jack", "{}", info);});
	jack::set_error_callback(|error| {warn!(target: "jack", "{}", error);});
	
	let matches = App::new(crate_name!())
		.version(crate_version!())
		.author(crate_authors!())
		.about(crate_description!())
		.setting(AppSettings::ArgRequiredElseHelp)
		.setting(AppSettings::DontCollapseArgsInUsage)
		.arg(Arg::from_usage("[send] -s, --send-to=[ADDRESS:PORT] 'Enable MIDI->Network transport'")
			.min_values(0)
			.max_values(1)
			.default_value_if("send", None, DEFAULT_SEND_ADDRESS))
		.arg(Arg::from_usage("[receive] -r, --receive-from=[PORT] 'Enable Network->MIDI transport'")
			.min_values(0)
			.max_values(1)
			.default_value_if("receive", None, DEFAULT_RECEIVE_ADDRESS))
		.arg(Arg::from_usage("[name] -n, --name=[CLIENT NAME] 'Name the JACK client to avoid collisions with multiple instances'")
			.default_value(crate_name!()))
		.arg(Arg::from_usage("[count] -c, --count=[NUMBER] 'Number of MIDI ports to create (Max 255)'")
			.default_value("1"))
		.group(ArgGroup::with_name("io")
			.args(&["send", "receive"])
			.multiple(true)
			.required(true))
		.get_matches();
	
	let count = value_t!(matches.value_of("count"), u8).unwrap_or_else(|e| e.exit());
	let receive_address = matches.value_of("receive")
			.or(if matches.is_present("receive") {Some(DEFAULT_RECEIVE_ADDRESS)} else {None});
	let send_address = matches.value_of("send")
			.or(if matches.is_present("send") {Some(DEFAULT_SEND_ADDRESS)} else {None});
	
	// Bi-directional channels
	let (tx_input, rx_input) = mpsc::channel();
	let (tx_output, rx_output) = mpsc::channel();
	
	// Network receive handler
	let receive_handler = receive_address.map(|address| {
		let receive_socket = UdpSocket::bind(address).expect("Can't create UDP socket");
		info!("Receive mode activated. Listening on {}.", address);

		move || {
			let mut buffer = vec![0;decoder::MTU];
			loop {
				receive_socket.recv_from(&mut buffer)
					.map_or_else(|e| {error!("Could not receive UDP packet: {}", e); None}, |v| Some(v))
					.map(|(read, _sender)| decoder::decode(&mut buffer[..read]))
					.transpose()
					.unwrap_or_else(|e| {error!("Could not decode OSC packet: {:?}", e); None})
					.map_or(Vec::new().into_iter(), |message| match message {
						OscPacket::Message(OscMessage{addr, args}) => if addr == "/midi" {
							if let Some(data) = args {
								data.into_iter()
							} else {
								Vec::new().into_iter()
							}
						} else {
							Vec::new().into_iter()
						},
						_ => Vec::new().into_iter() // Simply ignore non-matching packets (Should we? Or should there be a warning?)
					})
					.filter_map(|osc_type| {
						if let OscType::Midi(midi_message) = osc_type {
							Some(midi_message)
						} else {
							None
						}
					})
					.try_for_each(|midi_message| tx_output.send(midi_message)).into_result()
					.err()
					.map(|e| {error!("Could not send message down the pipe: {}", e);});
			}
		}
	});

	// Network send handler
	let send_handler = send_address.map(|address| {
		let send_socket = UdpSocket::bind(DEFAULT_BIND_ADDRESS).expect("Can't create UDP socket");
		send_socket.connect(address).expect("Can't connect to UDP socket");
		info!("Send mode activated. Sending to {}.", address);

		move || {
			for message in rx_input.iter() {
				let buffer = encoder::encode(&OscPacket::Message(OscMessage {
					addr: "/midi".to_string(),
					args: Some(vec![OscType::Midi(message)])
				})).expect("Can't encode message");
				if let Err(e) = send_socket.send(&buffer) {
					error!("Could not send UDP packet: {}", e);
				}
			}
		}
	});

	// JACK handler

	let (client, _status) = Client::new(&value_t!(matches.value_of("name"), String).unwrap_or_else(|e| e.exit()), jack::ClientOptions::NO_START_SERVER).expect("Could not connect to JACK server");
	let input_ports: Vec<Port<MidiIn>> = match send_address {
		Some(_) => (0..count).map(|i| client.register_port(&format!("input_{}", i), jack::MidiIn::default()).expect("Could not create MIDI input port")).collect(),
		None => Vec::new()
	};
	let output_ports: Vec<Port<MidiOut>> = match receive_address {
		Some(_) => (0..count).map(|i| client.register_port(&format!("output_{}", i), jack::MidiOut::default()).expect("Could not create MIDI output port")).collect(),
		None => Vec::new()
	};

	let jack_handler = JackHandler {
		input_ports, output_ports, tx_input, rx_output
	};
	
	// Start the threads
	
	let _client = client.activate_async((), jack_handler).expect("Could not start JACK client");

	let mut handler_threads = Vec::new();
	if let Some(handler) = receive_handler {
		handler_threads.push(std::thread::spawn(handler));
	}
	if let Some(handler) = send_handler {
		handler_threads.push(std::thread::spawn(handler));
	}
	
	// Wait for them
	for t in handler_threads {
		t.join().expect("Failed waiting for remaining threads");
	}
}

struct JackHandler {
	input_ports: Vec<Port<MidiIn>>,
	output_ports: Vec<Port<MidiOut>>,
	tx_input: mpsc::Sender<OscMidiMessage>,
	rx_output: mpsc::Receiver<OscMidiMessage>
}

impl ProcessHandler for JackHandler {
	fn process(&mut self, _client: &Client, process_scope: &ProcessScope) -> jack::Control {
		for (p, port) in &mut self.input_ports.iter_mut().enumerate() {
			for event in port.iter(process_scope) {
				self.tx_input.send(OscMidiMessage{
					port: p as u8,
					status: event.bytes[0],
					data1: event.bytes[1],
					data2: event.bytes[2]
				}).unwrap();
			}
		}
		let mut writers: Vec<jack::MidiWriter> = self.output_ports.iter_mut()
			.map(|port| port.writer(process_scope))
			.collect();
		for message in self.rx_output.try_iter() {
			&mut writers[message.port as usize].write(&RawMidi {
				time: 0,
				bytes: &[message.status, message.data1, message.data2]
			}).unwrap();
		}
		jack::Control::Continue
	}
}

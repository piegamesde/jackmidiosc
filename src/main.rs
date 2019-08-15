use std::net::UdpSocket;
use std::sync::mpsc;

extern crate jack;
use jack::{Client, Port, MidiIn, MidiOut, RawMidi, MidiWriter, ProcessHandler, ProcessScope};

#[macro_use]
extern crate clap;
use clap::{App, AppSettings, Arg, ArgGroup};

extern crate rosc;
use rosc::{OscPacket, OscMessage, OscMidiMessage, OscType, encoder, decoder};

fn main() {
	let matches = App::new(crate_name!())
		.version(crate_version!())
		.author(crate_authors!())
		.about(crate_description!())
		.setting(AppSettings::ArgRequiredElseHelp)
		.setting(AppSettings::DontCollapseArgsInUsage)
		.arg(Arg::from_usage("[send] -s, --send-to 'Enable MIDI->Network transport'")
			.value_name("[ADDRESS:]PORT")
			.min_values(0)
			.max_values(1)
			.default_value_if("send", None, "localhost:8953"))
		.arg(Arg::from_usage("[receive] -r, --receive-from 'Enable Network->MIDI transport'")
			.value_name("PORT")
			.min_values(0)
			.max_values(1)
			.default_value_if("receive", None, "8953"))
		.arg(Arg::from_usage("[name] -n, --name=[CLIENT NAME] 'Name the JACK client to avoid collisions with multiple instances'")
			.default_value(crate_name!()))
		.arg(Arg::from_usage("[count] -c, --count=[NUMBER] 'Number of MIDI ports to create (Max 255)'")
			.default_value("1"))
		.group(ArgGroup::with_name("io")
			.args(&["send", "receive"])
			.multiple(true)
			.required(true))
		.get_matches();
	
	let send = matches.is_present("send");
	let receive = matches.is_present("receive");
	
	let (client, _status) = Client::new(matches.value_of("name").unwrap(), jack::ClientOptions::NO_START_SERVER).unwrap();
	let count = matches.value_of("count").unwrap();
	let input_ports: Vec<Port<MidiIn>> = (0..5).map(|i| client.register_port(&format!("input_{}", i), jack::MidiIn::default()).unwrap()).collect();
	let output_ports: Vec<Port<MidiOut>> = (0..5).map(|i| client.register_port(&format!("output_{}", i), jack::MidiOut::default()).unwrap()).collect();

	// Bi-directional channels
	let (tx_input, rx_input) = mpsc::channel();
	let (tx_output, rx_output) = mpsc::channel();

	let jack_handler = JackHandler {
		input_ports, output_ports, tx_input, rx_output
	};
	
	let client = client.activate_async((), jack_handler).unwrap();
	
	// Network receive handler
	std::thread::spawn(move || {
		let socket = UdpSocket::bind("localhost:8953").expect("Can't create UDP socket");
		let mut buffer = vec![0;decoder::MTU];
		loop {
			let (read, _sender) = socket.recv_from(&mut buffer).unwrap();
			println!("Received {:?}", decoder::decode(&mut buffer).unwrap());
			
			match decoder::decode(&mut buffer[..read]).unwrap() {
				OscPacket::Message(OscMessage{addr, args}) => if addr == "/midi" {
					if let Some(data) = args {
						for osc_type in data.iter() {
							if let OscType::Midi(midi_message) = osc_type {
								tx_output.send(midi_message.clone());
							}
						}
					}
				},
				_ => ()
			}
		}
	});
	
	// Network send handler
	let socket = UdpSocket::bind("0.0.0.0:0").expect("Can't create UDP socket");
	socket.connect("localhost:8953").unwrap();
	for message in rx_input.iter() {
		println!("Sending {:?}", message);
		let buffer = encoder::encode(&OscPacket::Message(OscMessage {
			addr: "/midi".to_string(),
			args: Some(vec![OscType::Midi(message)])
		})).expect("Can't encode message");
		println!("Sending {:?}", buffer);
		socket.send(&buffer).unwrap();
	}
}

struct JackHandler {
	input_ports: Vec<Port<MidiIn>>,
	output_ports: Vec<Port<MidiOut>>,
	tx_input: mpsc::Sender<OscMidiMessage>,
	rx_output: mpsc::Receiver<OscMidiMessage>
}

impl ProcessHandler for JackHandler {
	fn process(&mut self, client: &Client, process_scope: &ProcessScope) -> jack::Control {
		for port in (0..self.input_ports.len()) {
			for event in self.input_ports[port].iter(process_scope) {
				println!("Input {:?}", event);
				self.tx_input.send(OscMidiMessage{
					port: port as u8,
					status: event.bytes[0],
					data1: event.bytes[1],
					data2: event.bytes[2]
				});
			}
		}
		let mut data: Vec<Vec<u8>> = vec![Vec::with_capacity(3); self.output_ports.len()];
		for message in self.rx_output.try_iter() {
			data[message.port as usize].push(message.status);
			data[message.port as usize].push(message.data1);
			data[message.port as usize].push(message.data2);
		}
		for port in (0..self.output_ports.len()) {
			let mut writer = self.output_ports[port].writer(process_scope);
			if (data[port].len() > 0) {
			println!("Output {:?}", RawMidi {
				time: 0,
				bytes: &data[port]
			});
			}
			writer.write(&RawMidi {
				time: 0,
				bytes: &data[port]
			}).unwrap();
		}
		jack::Control::Continue
	}
}

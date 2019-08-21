# jackmidiosc

Midi to OSC bridge using JACK audio. It can be used to tunnel MIDI connections over the internet.

In send mode, it will wrap all MIDI events into OSC events. In receive mode, it will do the opposite.

The OSC path for all events is `/midi`. Up to 255 ports per instance are supported.

## Compiling

Clone and run it from Cargo:

```bash
git clone https://github.com/piegamesde/jackmidiosc
cd jackmidiosc
# Run directly from cargo
cargo run -- $args # put your args here
# Alternatively: Build and run the binary
cargo build --release
./target/release/jackmidiosc $args
```

At the moment, you need to have the Rust nightly compiler installed.

## Usage

You can send incoming MIDI data to an OSC server with:
```
jackmidiosc -s
jackmidiosc --send
jackmidiosc -s <IP:PORT>
```
You can receive incoming OSC data with:
```
jackmidiosc -r
jackmidiosc --receive
jackmidiosc -r localhost:<PORT>
```

See `jackmidiosc --help` for the available options.

To try if it is working, run `jackmidiosc -s -r`. This will create a JACK client with one input and output. Pass some MIDI data into the input and it should get forwarded to the output without modification.

Once started, the application will run indefinitely until it is interrupted with `Ctrl+C` or an error occurs. Networking errors will be caught and logged, JACK errors (e.g. stopping the server) will crash the application.

## Similar software

`jackmidiosc` is inspided by:

- [midioscar](https://github.com/rrbone/midioscar)
- [MidiOSC](https://github.com/jstutters/MidiOSC)
- [JackTrip](https://github.com/jcacerec/jacktrip)

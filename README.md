# jackmidiosc

Midi to OSC bridge using JACK audio. It can be used to tunnel MIDI connections over the internet.

In send mode, it will wrap all MIDI events into OSC events. In receive mode, it will do the opposite.

The OSC path for all events is `/midi`. Up to 255 ports per instance are supported.

## Similar software

`jackmidiosc` is inspided by:

- [midioscar](https://github.com/rrbone/midioscar)
- [MidiOSC](https://github.com/jstutters/MidiOSC)
- [JackTrip](https://github.com/jcacerec/jacktrip)

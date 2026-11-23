# sensors
A Raspberry Pi's own sensors, through `vcgencmd`: the SoC temperature and
whether it was throttled (too hot, or under-voltage). Read the snapshot;
`sensors.read` reads it again now.

probe: vcgencmd version
snapshot: vcgencmd measure_temp
interval: 30s

## tool: read
The SoC temperature now.
command: vcgencmd measure_temp

## tool: throttled
Whether the Pi was throttled since boot: 0x0 is never.
command: vcgencmd get_throttled

## event: hot
when: temp > 70
The SoC is above 70°C: it will throttle soon.

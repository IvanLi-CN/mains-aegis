# UMW AO3400A — key parameters (project use: CLM heater NMOS)

Package: `SOT-23`  
Pinout: `1=G, 2=S, 3=D`  
Type: `N-Channel MOSFET`

Project use: the heater switch for `CLM1612P1412` (secondary protection fuse / CLM),
where the gate drive can be as low as a couple volts due to the resistor divider.

## Key specs (TA = 25°C)

- `VDS = 30 V`
- `VGS = ±12 V`
- `ID = 5.8 A` (continuous)
- `VGS(th) typ = 1.4 V` (at `ID = 250 µA`)
- `RDS(on) max = 27 mΩ` @ `VGS = 10 V`, `ID = 5.8 A`
- `RDS(on) max = 31 mΩ` @ `VGS = 4.5 V`, `ID = 5 A`
- `RDS(on) max = 48 mΩ` @ `VGS = 2.5 V`, `ID = 4 A` (**critical for this design**)

Source PDF (UMW official): https://www.umw-ic.com/static/pdf/08026a9ce7af7458f6634e46da45de75.pdf


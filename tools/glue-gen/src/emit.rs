//! Writes one part's routing glue: `src/chips/gen/<part>.rs`, macro rows
//! the part's chip module includes beside its hand facts
//! (`docs/design/glue-generator.md`).
//!
//! Everything here comes from the die table, cut to the part's device (which
//! peripherals it enables) and package (which pins it bonds). What metapac
//! does not say is either a hand fact in the chip module or offering policy,
//! below: which IP versions FerroHAL drives, and which instances it offers.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use glue_model::PeripheralDma;

use crate::registry::Part;
use crate::silicon::{Die, DiePeripheral};

/// The family whose macro shapes a part's rows take. The F7 takes the F4's:
/// fixed-request stream DMA and bus-named RCC registers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    /// Fixed-request DMA: a route is (controller, stream, channel). The F4
    /// and the F7.
    F4,
    /// DMAMUX1: any DMA1 or DMA2 stream serves a request ID.
    H7,
}

/// Whether FerroHAL drives this IP version on this series. The F7's usart_v3
/// is served by the usart_v4 module (ADR-011).
pub fn drives(series: &str, kind: &str, version: &str) -> bool {
    matches!(
        (series, kind, version),
        ("F4", "usart", "v2")
            | ("F4", "spi", "v2_i2s")
            | ("F4", "i2c", "v1")
            | ("F4", "timer", "v1")
            | ("F4", "adc", "v2")
            | ("F7", "usart", "v3")
            | ("F7", "spi", "v3_i2s")
            | ("F7", "i2c", "v2")
            | ("F7", "timer", "v1")
            | ("F7", "adc", "v2")
            | ("H7", "usart", "v4")
            | ("H7", "spi", "v4_i2s")
            | ("H7", "i2c", "v2")
            | ("H7", "timer", "v3")
            | ("H7", "adc", "v4")
            | ("H7", "sdmmc", "v2")
            | ("F4" | "F7" | "H7", "otg", "v1")
    )
}

/// The macro shapes a series' rows take.
fn family(series: &str) -> Result<Family, String> {
    match series {
        "F4" | "F7" => Ok(Family::F4),
        "H7" => Ok(Family::H7),
        other => Err(format!("no macro shapes for series {other}")),
    }
}

/// Offering policy beyond the IP version, with its reason.
fn offered(family: Family, name: &str, peripheral: &DiePeripheral) -> bool {
    match peripheral.kind.as_deref() {
        // LPUART shares usart_v4's version but is its own block, which the
        // UART driver does not carry.
        Some("usart") => peripheral.block.as_deref() == Some("USART"),
        // The ADC driver enables and resets its instance's RCC bit on open.
        // The F4's and F7's ADC2 and ADC3 have no reset of their own
        // (ADCRST, ADC1's bit, resets all three), and the H7's ADC2 shares ADC12EN with
        // ADC1: opening one would reset the other mid-conversion.
        Some("adc") => match family {
            Family::F4 => name == "ADC1",
            Family::H7 => name == "ADC1" || name == "ADC3",
        },
        // The full-speed port only: the high-speed core needs a ULPI PHY or
        // its own pins, which the USB driver does not carry.
        Some("otg") => name == "USB_OTG_FS",
        _ => true,
    }
}

/// `APB1LENR` -> `Apb1L`: FerroHAL's `Bus` variant for an enable register.
fn bus(register: &str) -> String {
    let stem = register.trim_end_matches("ENR");
    let mut out = String::new();
    let mut previous_digit = false;
    for (i, c) in stem.chars().enumerate() {
        if i == 0 || previous_digit {
            out.push(c.to_ascii_uppercase());
        } else {
            out.push(c.to_ascii_lowercase());
        }
        previous_digit = c.is_ascii_digit();
    }
    out
}

/// `PA10` -> ('A', 10); `None` for an analog pad (`PC2_C`).
fn pin(name: &str) -> Option<(char, u8)> {
    let rest = name.strip_prefix('P')?;
    let port = rest.chars().next()?;
    rest[1..].parse().ok().map(|n| (port, n))
}

/// Natural order: `TIM2` before `TIM10`.
fn natural(name: &str) -> (String, u32) {
    let digits: String = name
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect();
    let stem = &name[..name.len() - digits.len()];
    (
        stem.to_string(),
        digits
            .chars()
            .rev()
            .collect::<String>()
            .parse()
            .unwrap_or(0),
    )
}

fn sorted<'a>(names: impl Iterator<Item = &'a String>) -> Vec<&'a String> {
    let mut v: Vec<&String> = names.collect();
    v.sort_by_key(|n| natural(n));
    v
}

struct Cut<'a> {
    family: Family,
    part: &'a Part,
    die: &'a Die,
    bonded: &'a BTreeSet<String>,
    enabled: &'a BTreeSet<String>,
    /// The cut's (peripheral, pin, signal) rows.
    rows: &'a BTreeSet<(String, String, String)>,
}

impl<'a> Cut<'a> {
    fn peripheral(&self, name: &str) -> Option<&'a DiePeripheral> {
        self.enabled
            .contains(name)
            .then(|| self.die.peripherals.get(name))
            .flatten()
    }

    /// The part's driven, offered instances of `kind`, in natural order.
    fn instances(&self, kind: &str) -> Vec<(&'a String, &'a DiePeripheral)> {
        let mut out: Vec<(&String, &DiePeripheral)> = self
            .die
            .peripherals
            .iter()
            .filter(|(name, p)| {
                self.enabled.contains(*name)
                    && p.kind.as_deref() == Some(kind)
                    && drives(self.part.series, kind, p.version.as_deref().unwrap_or(""))
                    && offered(self.family, name, p)
            })
            .collect();
        out.sort_by_key(|(name, _)| natural(name));
        out
    }

    /// Whether the part enables any peripheral of `kind`, driven or not.
    fn present(&self, kind: &str) -> bool {
        self.die
            .peripherals
            .iter()
            .any(|(name, p)| self.enabled.contains(name) && p.kind.as_deref() == Some(kind))
    }

    /// The bonded pins carrying `peripheral`'s `signal` in this cut, as
    /// `('A', 9, 7)` rows.
    fn pins(&self, peripheral: &str, p: &DiePeripheral, signal: &str) -> Vec<(char, u8, u8)> {
        let mut rows: Vec<(char, u8, u8)> = p
            .pins
            .iter()
            .filter(|((name, s), _)| {
                s == signal && self.bonded.contains(name) && self.has_row(peripheral, name, s)
            })
            .filter_map(|((name, _), af)| Some((pin(name)?.0, pin(name)?.1, (*af)?)))
            .collect();
        rows.sort();
        rows.dedup();
        rows
    }

    fn has_row(&self, peripheral: &str, pin: &str, signal: &str) -> bool {
        self.rows
            .contains(&(peripheral.to_string(), pin.to_string(), signal.to_string()))
    }

    fn vector(&self, p: &'a DiePeripheral, signal: &str) -> Option<&'a String> {
        p.interrupts
            .get(&(self.part.device.to_string(), signal.to_string()))
    }

    /// F4 routes for `signal`: (controller, stream, channel). Where one
    /// stream serves the signal through two channel selections, the one
    /// serving the fewest signals on that stream is kept (TIM1 CH3 on DMA2
    /// stream 6: channel 6, not channel 0, which CH1 to CH3 share).
    fn routes(&self, p: &DiePeripheral, signal: &str) -> Vec<(u8, u8, u32)> {
        let mut by_stream: BTreeMap<(u8, u8), Vec<u32>> = BTreeMap::new();
        for route in p.dma.iter().filter(|r| r.signal == signal) {
            if let Some(stream) = route.channel.as_deref().and_then(stream_of) {
                by_stream
                    .entry(stream)
                    .or_default()
                    .push(route.request.unwrap_or(0));
            }
        }
        by_stream
            .into_iter()
            .map(|(stream, mut requests)| {
                requests.sort_by_key(|request| (self.sharing(stream, *request), *request));
                (stream.0, stream.1, requests[0])
            })
            .collect()
    }

    /// How many (peripheral, signal) pairs one stream and channel selection serve.
    fn sharing(&self, stream: (u8, u8), request: u32) -> usize {
        self.die
            .peripherals
            .values()
            .flat_map(|p| p.dma.iter())
            .filter(|r| {
                r.channel.as_deref().and_then(stream_of) == Some(stream)
                    && r.request == Some(request)
            })
            .count()
    }

    /// The DMAMUX1 request for `signal`, the H7's.
    fn mux(&self, p: &DiePeripheral, signal: &str) -> Option<u32> {
        p.dma
            .iter()
            .filter(|r: &&PeripheralDma| {
                r.signal == signal && r.dmamux.as_deref() == Some("DMAMUX1")
            })
            .filter_map(|r| r.request)
            .min()
    }
}

/// `DMA2_CH7` -> (2, 7). BDMA's channels are not offered.
fn stream_of(channel: &str) -> Option<(u8, u8)> {
    let rest = channel.strip_prefix("DMA")?;
    let (controller, stream) = rest.split_once("_CH")?;
    Some((controller.parse().ok()?, stream.parse().ok()?))
}

fn rcc(p: &DiePeripheral) -> Result<(String, u32), String> {
    let enable = p
        .rcc
        .as_ref()
        .and_then(|r| r.enable.as_ref())
        .ok_or("no RCC enable")?;
    Ok((
        bus(&enable.register),
        enable.bit.ok_or("RCC enable bit unresolved")?,
    ))
}

fn rows(rows: &[(char, u8, u8)]) -> String {
    rows.iter()
        .map(|(p, n, af)| format!("('{p}', {n}, {af})"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The H7's kernel-clock muxes, as reset leaves them: `None` is the bus clock.
fn reset_kernel(mux: &str) -> Option<&'static str> {
    match mux {
        "SPI123SEL" | "SDMMCSEL" => Some("Pll1Q"),
        _ => None,
    }
}

pub fn emit(die: &Die, part: &Part, metapac: &str) -> Result<String, String> {
    let family = family(part.series)?;
    let bonded = die
        .packages
        .get(part.package)
        .ok_or("package not in the die table")?;
    let key = (part.device.to_string(), part.package.to_string());
    let enabled = die
        .cuts
        .get(&key)
        .ok_or("device and package not in the die table")?;
    let cut_rows = die
        .rows
        .get(&key)
        .ok_or("device and package not in the die table")?;
    let cut = Cut {
        family,
        part,
        die,
        bonded,
        enabled,
        rows: cut_rows,
    };
    let mut out = String::new();
    let w = &mut out;
    let fail = |name: &str, e: &str| format!("{}: {name}: {e}", part.feature);

    writeln!(
        w,
        "//! {}'s routing glue, generated by tools/glue-gen from stm32-metapac {metapac}:",
        part.feature
    )
    .ok();
    writeln!(
        w,
        "//! die {:#05x}, device {}, package {}. Do not edit: `glue-gen emit part {}`",
        part.die, part.device, part.package, part.feature
    )
    .ok();
    writeln!(w, "//! rewrites it.").ok();
    writeln!(w, "//!").ok();
    writeln!(w, "//! Requirements: GLUE-001, GLUE-002").ok();
    writeln!(w, "\nuse super::*;").ok();

    // Tokens: every peripheral the device enables, but the GPIO ports and the
    // DMA controllers with their DMAMUX (typed below), and RCC, FLASH and
    // PWR, which `freeze` owns.
    let core = |name: &str| {
        name.starts_with("GPIO")
            || matches!(name, "DMA1" | "DMA2" | "DMAMUX1" | "RCC" | "FLASH" | "PWR")
    };
    let tokens = sorted(enabled.iter().filter(|n| !core(n)));
    writeln!(w, "\ntokens!(").ok();
    for t in &tokens {
        writeln!(w, "    {t},").ok();
    }
    writeln!(w, ");").ok();

    let mut ports: BTreeMap<char, u16> = BTreeMap::new();
    for name in bonded {
        if let Some((port, n)) = pin(name) {
            *ports.entry(port).or_default() |= 1 << n;
        }
    }
    let dmas: Vec<&String> = sorted(enabled.iter().filter(|n| {
        n.starts_with("DMA") && !n.starts_with("DMAMUX") && n[3..].parse::<u8>().is_ok()
    }));

    writeln!(w, "\nperipherals!(").ok();
    writeln!(w, "    \"Not here: FLASH and PWR, which RCC's `freeze` owns for the flash latency and the voltage scale.\";").ok();
    writeln!(w, "    RCC: RCC,").ok();
    for port in ports.keys() {
        writeln!(w, "    GPIO{port}: Port<'{port}'>,").ok();
    }
    for d in &dmas {
        writeln!(w, "    {d}: Dma<{}>,", &d[3..]).ok();
    }
    for t in &tokens {
        writeln!(w, "    {t}: {t},").ok();
    }
    writeln!(w, ");").ok();

    writeln!(w, "\nports!(").ok();
    for port in ports.keys() {
        let name = format!("GPIO{port}");
        let p = cut
            .peripheral(&name)
            .ok_or_else(|| fail(&name, "bonded but not enabled"))?;
        let (_, bit) = rcc(p).map_err(|e| fail(&name, &e))?;
        writeln!(w, "    '{port}' => {name}, {bit};").ok();
    }
    writeln!(w, ");").ok();
    writeln!(w, "\nport_parts!(").ok();
    for (port, mask) in &ports {
        let fields: Vec<String> = (0..16)
            .filter(|n| mask & (1 << n) != 0)
            .map(|n| format!("p{n}: {n}"))
            .collect();
        writeln!(w, "    '{port}' => Parts{port} [{}];", fields.join(", ")).ok();
    }
    writeln!(w, ");").ok();

    writeln!(w, "\ndmas!(").ok();
    for d in &dmas {
        let p = cut.peripheral(d).ok_or_else(|| fail(d, "missing"))?;
        let (bus, bit) = rcc(p).map_err(|e| fail(d, &e))?;
        match family {
            Family::F4 => writeln!(w, "    {} => {d}, {bus}, {bit};", &d[3..]),
            Family::H7 => {
                let base = die
                    .dma_channels
                    .get(&format!("{d}_CH0"))
                    .and_then(|c| c.dmamux_channel)
                    .ok_or_else(|| fail(d, "stream 0 has no DMAMUX channel"))?;
                writeln!(w, "    {} => {d}, {bus}, {bit}, mux {base};", &d[3..])
            }
        }
        .ok();
    }
    writeln!(w, ");").ok();

    // UARTs.
    let usarts = cut.instances("usart");
    // A series whose USART version has no driver gets no UART rows: the chip
    // module leaves the UART out.
    if !usarts.is_empty() {
        writeln!(w, "\nusarts!(").ok();
        for (name, p) in &usarts {
            let (bus, bit) = rcc(p).map_err(|e| fail(name, &e))?;
            let kind = if name.starts_with("UART") {
                "uart"
            } else {
                "usart"
            };
            match family {
                Family::F4 => writeln!(w, "    {kind} {name} => {name}_INFO, {bus}, {bit};"),
                Family::H7 => writeln!(w, "    {name} => {name}_INFO, {bus}, {bit};"),
            }
            .ok();
        }
        writeln!(w, ");").ok();
        pin_rows(w, &cut, &usarts, &[("TX", "TxPin"), ("RX", "RxPin")], None);
        dma_rows(w, &cut, &usarts, ("RX", "RxDma"), ("TX", "TxDma"));
    }

    // SPIs.
    let spis = cut.instances("spi");
    writeln!(w, "\nspis!(").ok();
    for (name, p) in &spis {
        let (bus, bit) = rcc(p).map_err(|e| fail(name, &e))?;
        let kernel = match p.rcc.as_ref().map(|r| &r.kernel_clock) {
            Some(glue_model::KernelClock::Mux(field)) => reset_kernel(&field.field)
                .map(str::to_string)
                .unwrap_or(bus.clone()),
            _ => bus.clone(),
        };
        writeln!(w, "    {name} => {name}_INFO, {bus}, {bit}, {kernel};").ok();
    }
    writeln!(w, ");").ok();
    pin_rows(
        w,
        &cut,
        &spis,
        &[("SCK", "SckPin"), ("MISO", "MisoPin"), ("MOSI", "MosiPin")],
        None,
    );
    dma_rows(w, &cut, &spis, ("RX", "SpiRxDma"), ("TX", "SpiTxDma"));

    // I2Cs.
    let i2cs = cut.instances("i2c");
    writeln!(w, "\ni2cs!(").ok();
    for (name, p) in &i2cs {
        let (bus, bit) = rcc(p).map_err(|e| fail(name, &e))?;
        writeln!(w, "    {name} => {name}_INFO, {bus}, {bit};").ok();
    }
    writeln!(w, ");").ok();
    // I2C lines are shared and pulled up: only the open-drain form routes.
    pin_rows(
        w,
        &cut,
        &i2cs,
        &[("SCL", "SclPin"), ("SDA", "SdaPin")],
        Some("OpenDrain"),
    );
    dma_rows(w, &cut, &i2cs, ("RX", "I2cRxDma"), ("TX", "I2cTxDma"));

    // Timers.
    let timers = cut.instances("timer");
    writeln!(w, "\ntimers!(").ok();
    for (name, p) in &timers {
        let (bus, bit) = rcc(p).map_err(|e| fail(name, &e))?;
        let update = cut
            .vector(p, "UP")
            .ok_or_else(|| fail(name, "no UP vector"))?;
        let bits = if p.block.as_deref() == Some("TIM_GP32") {
            32
        } else {
            16
        };
        writeln!(w, "    {name} => {bus}, {bit}, {update}, {bits};").ok();
    }
    writeln!(w, ");").ok();
    writeln!(w, "\nchannel_timers!(").ok();
    for (name, p) in &timers {
        let shape = match p.block.as_deref() {
            Some("TIM_ADV") => Some((4, "break")),
            Some("TIM_GP16" | "TIM_GP32") => Some((4, "plain")),
            Some("TIM_2CH") => Some((2, "plain")),
            Some("TIM_1CH") => Some((1, "plain")),
            Some("TIM_2CH_CMP") => Some((2, "break")),
            Some("TIM_1CH_CMP") => Some((1, "break")),
            Some("TIM_BASIC") => None,
            other => return Err(fail(name, &format!("unknown timer block {other:?}"))),
        };
        if let Some((channels, kind)) = shape {
            writeln!(w, "    {name} => {channels}, {kind};").ok();
        }
    }
    writeln!(w, ");\n").ok();
    for (name, p) in &timers {
        for signal in ["CH1", "CH1N", "CH2", "CH2N", "CH3", "CH3N", "CH4", "CH4N"] {
            let r = cut.pins(name, p, signal);
            if !r.is_empty() {
                writeln!(w, "timer_pins!({name} {signal}: {});", rows(&r)).ok();
            }
        }
    }
    writeln!(w).ok();
    for (name, p) in &timers {
        for signal in ["CH1", "CH2", "CH3", "CH4", "UP"] {
            match family {
                Family::F4 => {
                    let r = cut.routes(p, signal);
                    if !r.is_empty() {
                        let list: Vec<String> = r
                            .iter()
                            .map(|(d, s, q)| format!("({d}, {s}, {q})"))
                            .collect();
                        writeln!(w, "timer_dma!({name} {signal}: {});", list.join(", ")).ok();
                    }
                }
                Family::H7 => {
                    if let Some(request) = cut.mux(p, signal) {
                        writeln!(w, "timer_dma!({name} {signal}: mux {request});").ok();
                    }
                }
            }
        }
    }

    // ADCs.
    let adcs = cut.instances("adc");
    writeln!(w, "\nadcs!(").ok();
    for (name, p) in &adcs {
        let (bus, bit) = rcc(p).map_err(|e| fail(name, &e))?;
        let digit = &name[3..];
        let common = sorted(enabled.iter().filter(|n| {
            n.starts_with("ADC") && n.ends_with("_COMMON") && n[3..n.len() - 7].contains(digit)
        }));
        let common = common
            .first()
            .ok_or_else(|| fail(name, "no common block"))?;
        writeln!(w, "    {name} => {common}, {bus}, {bit};").ok();
    }
    writeln!(w, ");").ok();
    for (name, p) in &adcs {
        let mut channels: Vec<(char, u8, u8)> = p
            .pins
            .keys()
            .filter(|(pin_name, signal)| {
                bonded.contains(pin_name) && cut.has_row(name, pin_name, signal)
            })
            .filter_map(|(pin_name, signal)| {
                let n = signal
                    .strip_prefix("INP")
                    .or_else(|| signal.strip_prefix("IN"))?
                    .parse()
                    .ok()?;
                let (port, number) = pin(pin_name)?;
                Some((port, number, n))
            })
            .collect();
        channels.sort();
        if !channels.is_empty() {
            writeln!(w, "adc_pins!({name}: {});", rows(&channels)).ok();
        }
        match family {
            Family::F4 => {
                let r = cut.routes(p, name);
                let list: Vec<String> = r
                    .iter()
                    .map(|(d, s, q)| format!("({d}, {s}, {q})"))
                    .collect();
                if !list.is_empty() {
                    writeln!(w, "adc_dma!({name}: {});", list.join(", ")).ok();
                }
            }
            Family::H7 => {
                if let Some(request) = cut.mux(p, name) {
                    writeln!(w, "adc_dma_mux!({name}: mux {request});").ok();
                }
            }
        }
    }

    // USB.
    for (name, p) in cut.instances("otg") {
        let (bus, bit) = rcc(p).map_err(|e| fail(name, &e))?;
        let vector = cut
            .vector(p, "GLOBAL")
            .ok_or_else(|| fail(name, "no vector"))?;
        // The port's data pins: one route each on every part here, so the
        // driver names them as types rather than taking routing traits.
        let pin = |signal: &str| match cut.pins(name, p, signal).as_slice() {
            [(port, pin, af)] => Ok(format!("('{port}', {pin}, {af})")),
            rows => Err(fail(
                name,
                &format!("{signal}: {} routes, not one", rows.len()),
            )),
        };
        let (dm, dp) = (pin("DM")?, pin("DP")?);
        writeln!(
            w,
            "\nusb_fs!({name} => {bus}, {bit}, {vector}; dm {dm}, dp {dp});"
        )
        .ok();
    }

    // SDMMC.
    let sdmmcs = cut.instances("sdmmc");
    if !sdmmcs.is_empty() {
        writeln!(w, "\nsdmmcs!(").ok();
        for (name, p) in &sdmmcs {
            let (bus, bit) = rcc(p).map_err(|e| fail(name, &e))?;
            writeln!(w, "    {name} => {bus}, {bit};").ok();
        }
        writeln!(w, ");").ok();
        pin_rows(
            w,
            &cut,
            &sdmmcs,
            &[
                ("CK", "CkPin"),
                ("CMD", "CmdPin"),
                ("D0", "D0Pin"),
                ("D1", "D1Pin"),
                ("D2", "D2Pin"),
                ("D3", "D3Pin"),
            ],
            None,
        );
    }

    // The vector each DMA stream raises.
    writeln!(w, "\nstream_interrupts!(").ok();
    for d in &dmas {
        let p = cut.peripheral(d).ok_or_else(|| fail(d, "missing"))?;
        let mut streams: Vec<String> = Vec::new();
        for n in 0..8 {
            let vector = cut
                .vector(p, &format!("CH{n}"))
                .ok_or_else(|| fail(d, "stream without a vector"))?;
            streams.push(format!("{n} => {vector}"));
        }
        writeln!(w, "    {}: [{}];", &d[3..], streams.join(", ")).ok();
    }
    writeln!(w, ");").ok();
    Ok(out)
}

fn pin_rows(
    w: &mut String,
    cut: &Cut,
    instances: &[(&String, &DiePeripheral)],
    signals: &[(&str, &str)],
    otype: Option<&str>,
) {
    writeln!(w).ok();
    let otype = otype.map(|t| format!(", {t}")).unwrap_or_default();
    for (name, p) in instances {
        for (signal, trait_name) in signals {
            let r = cut.pins(name, p, signal);
            if !r.is_empty() {
                writeln!(w, "pins!({trait_name}<{name}>{otype}: {});", rows(&r)).ok();
            }
        }
    }
}

fn dma_rows(
    w: &mut String,
    cut: &Cut,
    instances: &[(&String, &DiePeripheral)],
    rx: (&str, &str),
    tx: (&str, &str),
) {
    match cut.family {
        Family::F4 => {
            writeln!(w).ok();
            for (name, p) in instances {
                for (signal, trait_name) in [tx, rx] {
                    let r = cut.routes(p, signal);
                    if !r.is_empty() {
                        let list: Vec<String> = r
                            .iter()
                            .map(|(d, s, q)| format!("({d}, {s}, {q})"))
                            .collect();
                        writeln!(w, "dma!({trait_name}<{name}>: {});", list.join(", ")).ok();
                    }
                }
            }
        }
        Family::H7 => {
            writeln!(w, "\ndma!(\n    {}, {};", rx.1, tx.1).ok();
            for (name, p) in instances {
                if let (Some(r), Some(t)) = (cut.mux(p, rx.0), cut.mux(p, tx.0)) {
                    writeln!(w, "    {name}: rx {r}, tx {t};").ok();
                }
            }
            writeln!(w, ");").ok();
        }
    }
}

/// What a part offers, per driver: the instance lists `docs/CHIPS.md`'s
/// matrix restates, from the same cut and policy as the glue.
pub struct Offer {
    pub uarts: Vec<String>,
    pub spis_dma: Vec<String>,
    pub spis: Vec<String>,
    pub i2cs: Vec<String>,
    pub timers: Vec<String>,
    pub channel_timers: Vec<String>,
    pub complementary: Vec<String>,
    pub adcs: Vec<String>,
    pub usb: Vec<String>,
    pub sdmmcs: Vec<String>,
    pub dmas: Vec<String>,
    /// The kinds the part carries but FerroHAL does not drive in its IP
    /// version (the F4's and F7's SDIO): `–`, not n/a.
    pub undriven: Vec<&'static str>,
}

pub fn offer(die: &Die, part: &Part) -> Result<Offer, String> {
    let family = family(part.series)?;
    let bonded = die
        .packages
        .get(part.package)
        .ok_or("package not in the die table")?;
    let key = (part.device.to_string(), part.package.to_string());
    let enabled = die
        .cuts
        .get(&key)
        .ok_or("device and package not in the die table")?;
    let cut_rows = die
        .rows
        .get(&key)
        .ok_or("device and package not in the die table")?;
    let cut = Cut {
        family,
        part,
        die,
        bonded,
        enabled,
        rows: cut_rows,
    };
    let names = |v: Vec<(&String, &DiePeripheral)>| {
        v.into_iter().map(|(n, _)| n.clone()).collect::<Vec<_>>()
    };
    let timers = cut.instances("timer");
    let spis = cut.instances("spi");
    let spis_dma = spis
        .iter()
        .filter(|(_, p)| match family {
            Family::F4 => !cut.routes(p, "RX").is_empty() && !cut.routes(p, "TX").is_empty(),
            Family::H7 => cut.mux(p, "RX").is_some() && cut.mux(p, "TX").is_some(),
        })
        .map(|(n, _)| (*n).clone())
        .collect();
    Ok(Offer {
        uarts: names(cut.instances("usart")),
        spis_dma,
        spis: names(spis),
        i2cs: names(cut.instances("i2c")),
        channel_timers: timers
            .iter()
            .filter(|(_, p)| p.block.as_deref() != Some("TIM_BASIC"))
            .map(|(n, _)| (*n).clone())
            .collect(),
        complementary: timers
            .iter()
            .filter(|(_, p)| {
                matches!(
                    p.block.as_deref(),
                    Some("TIM_ADV" | "TIM_2CH_CMP" | "TIM_1CH_CMP")
                )
            })
            .map(|(n, _)| (*n).clone())
            .collect(),
        timers: names(timers),
        adcs: names(cut.instances("adc")),
        usb: names(cut.instances("otg")),
        sdmmcs: names(cut.instances("sdmmc")),
        dmas: sorted(
            enabled
                .iter()
                .filter(|n| matches!(n.as_str(), "DMA1" | "DMA2")),
        )
        .into_iter()
        .cloned()
        .collect(),
        undriven: ["usart", "spi", "i2c", "timer", "adc", "otg", "sdmmc"]
            .into_iter()
            .filter(|kind| cut.instances(kind).is_empty() && cut.present(kind))
            .collect(),
    })
}

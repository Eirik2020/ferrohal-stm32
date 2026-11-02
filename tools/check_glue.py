"""Checks FerroHAL's hand-written chip glue against stm32-metapac's metadata.

    python3 -I tools/check_glue.py              # every chip in src/chips
    python3 -I tools/check_glue.py stm32h753zi  # one chip
    python3 -I tools/check_glue.py stm32f401re --glue some/other/glue.rs

For each U(S)ART and SPI instance the glue routes, it compares with the
metadata of the stm32-metapac version Cargo.lock pins:

- its pins (TX and RX; SCK, MISO and MOSI) and their alternate functions,
  as exact sets;
- its RCC enable bit, resolved through metapac's register definitions;
- its DMA routes: (controller, stream, channel) on a fixed-request DMA, or
  the DMAMUX request IDs on a DMAMUX chip.

It also checks every GPIO port's and DMA controller's enable bit, each
controller's first DMAMUX channel, every timer's enable bit, update
interrupt vector and counter width, and every channel timer's channel count,
break unit, channel pins (CHx, CHxN) and compare and update DMA routes, and
the full-speed USB port's enable bit, vector and DM and DP pins. It also
checks each SPI's kernel clock, SYSCFG's enable bit, every DMA stream's vector,
the watchdog's peripheral, the EXTI line vectors `src/exti.rs` shares across
chips, each ADC's enable bit, pins (positive inputs) and DMA routes, and each
SDMMC's enable bit and pins. On a fixed-request DMA a stream that serves one signal on two
channels appears once in the glue, with either request. Instances the metadata
has and the glue leaves out are listed, not failed. Exits 1 on any mismatch.

The metadata doesn't hold the USB FIFO depth, endpoint count or core kind, the
clock limits, or the memory map: those stay checked by hand against the manuals.

The glue's macro rows are parsed as text, so their layout is part of the
contract: a row this script cannot parse fails the check rather than being
skipped.
"""

import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# A SPI's kernel clock in the metadata to the glue's column. The muxes are the
# choice `freeze` makes on the H7: SPI123SEL stays PLL1Q, SPI45SEL goes to PCLK2.
SPI_KERNEL = {"PCLK1": "Apb1", "PCLK2": "Apb2", "SPI123SEL": "Pll1Q", "SPI45SEL": "Apb2"}

# RCC enable register names (metadata) to the glue's `Bus` names.
BUS = {
    "AHB1ENR": "Ahb1",
    "AHB2ENR": "Ahb2",
    "AHB3ENR": "Ahb3",
    "AHB4ENR": "Ahb4",
    "APB1ENR": "Apb1",
    "APB1LENR": "Apb1L",
    "APB1HENR": "Apb1H",
    "APB2ENR": "Apb2",
    "APB3ENR": "Apb3",
    "APB4ENR": "Apb4",
}


def metapac_dir():
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--manifest-path", str(ROOT / "Cargo.toml")],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    for package in json.loads(out)["packages"]:
        if package["name"] == "stm32-metapac":
            return Path(package["manifest_path"]).parent
    sys.exit("stm32-metapac not found in cargo metadata")


class Metadata:
    """One chip's metadata, from metapac's generated Rust source."""

    def __init__(self, metapac, chip):
        chip_dir = metapac / "src" / "chips" / chip
        text = (chip_dir / "metadata.rs").read_text()
        include = re.search(r'include!\("\.\./(metadata_\d+\.rs)"\)', text)
        self.text = (metapac / "src" / "chips" / include.group(1)).read_text()
        pac = (chip_dir / "pac.rs").read_text()
        rcc = re.search(r'#\[path = "\.\./\.\./peripherals/(rcc_\w+\.rs)"\]\s*pub mod rcc;', pac)
        self.rcc_regs = (metapac / "src" / "peripherals" / rcc.group(1)).read_text()

    def peripheral(self, name):
        start = self.text.find(f'name: "{name}",')
        if start < 0:
            return None
        end = self.text.find("\n    Peripheral {", start)
        return self.text[start : end if end > 0 else len(self.text)]

    def names(self, kind):
        """Peripheral names whose register block is `kind` (say "usart")."""
        return sorted(
            set(
                re.findall(
                    r'Peripheral \{\s*name: "(\w+)",\s*address: \w+,\s*registers: Some\(\s*PeripheralRegisters \{\s*kind: "'
                    + kind
                    + '"',
                    self.text,
                )
            )
        )

    def pins(self, name):
        block = self.peripheral(name)
        return {
            (signal, pin, int(af))
            for pin, signal, af in re.findall(
                r'pin: "(\w+)",\s*signal: "(\w+)",\s*af: Some\((\d+)\)', block
            )
        }

    def analog_pins(self, name):
        """(signal, pin, None): analog inputs have no alternate function."""
        return {
            (signal, pin, None)
            for pin, signal in re.findall(r'pin: "(\w+)",\s*signal: "(\w+)"', self.peripheral(name))
        }

    def dma(self, name):
        """(signal, channel name or None, dmamux or None, request) tuples."""
        block = self.peripheral(name)
        routes = set()
        for signal, channel, dmamux, request in re.findall(
            r'signal: "(\w+)",\s*channel: (?:Some\("(\w+)"\)|None),\s*dmamux: (?:Some\("(\w+)"\)|None),'
            r"\s*remap: &\[[^\]]*\],\s*dma: (?:Some\(\"\w+\"\)|None),\s*request: Some\((\d+)\)",
            block,
        ):
            routes.add((signal, channel or None, dmamux or None, int(request)))
        return routes

    def enable(self, name):
        """(bus, bit) of the peripheral's RCC enable bit."""
        block = self.peripheral(name)
        m = re.search(
            r'enable: Some\(\s*PeripheralRccRegister \{\s*register: "(\w+)",\s*field: "(\w+)"', block
        )
        register, field = m.groups()
        return BUS[register], self.bit(register, field)

    def bit(self, register, field):
        struct = register[0] + register[1:].lower()
        start = self.rcc_regs.find(f"pub struct {struct}(")
        if start < 0:
            sys.exit(f"metapac RCC has no register {register}")
        m = re.compile(
            r"pub fn set_" + field.lower() + r"\(&mut self, val: bool\) \{\s*self\.0 = \(self\.0 & !\(0x01 << (\d+)usize\)\)"
        ).search(self.rcc_regs, start)
        if not m:
            sys.exit(f"metapac RCC register {register} has no field {field}")
        return int(m.group(1))

    def interrupt(self, name, signal):
        block = self.peripheral(name)
        m = re.search(r'signal: "' + signal + r'",\s*interrupt: "(\w+)"', block)
        return m.group(1) if m else None

    def block(self, name):
        return re.search(r'block: "(\w+)"', self.peripheral(name)).group(1)

    def kernel_clock(self, name):
        """A fixed clock's name, or the name of the mux field that picks one."""
        m = re.search(
            r'kernel_clock: (?:Clock\("(\w+)"\)|Mux\(\s*PeripheralRccRegister \{\s*register: "\w+",\s*field: "(\w+)")',
            self.peripheral(name),
        )
        return m.group(1) or m.group(2)

    def dmamux_channel(self, channel):
        m = re.search(r'name: "' + channel + r'",\s*dma: "\w+",\s*channel: \d+,\s*dmamux: (?:Some\("\w+"\)|None),\s*dmamux_channel: Some\((\d+)\)', self.text)
        return int(m.group(1)) if m else None


class Glue:
    """The facts a chip's glue file states, parsed from its macro rows."""

    def __init__(self, path):
        self.text = path.read_text()
        self.problems = []

    def rows(self, macro):
        """The bodies of every `macro!(...)` invocation."""
        bodies = re.findall(r"^" + macro + r"!\((.*?)\);", self.text, re.S | re.M)
        if not bodies:
            self.problems.append(f"no {macro}! invocation found")
        return bodies

    def usarts(self):
        found = {}
        for body in self.rows("usarts"):
            for name, bus, bit in re.findall(r"(\w+) => \w+, (\w+), (\d+);", body):
                found[name] = (bus, int(bit))
        return found

    def spis(self):
        found = {}
        if not re.search(r"^spis!\(", self.text, re.M):
            return found
        for body in self.rows("spis"):
            for name, bus, bit, _kernel in re.findall(r"(\w+) => \w+, (\w+), (\d+), (\w+);", body):
                found[name] = (bus, int(bit))
        return found

    def spi_kernels(self):
        found = {}
        if not re.search(r"^spis!\(", self.text, re.M):
            return found
        for body in self.rows("spis"):
            for name, _bus, _bit, kernel in re.findall(r"(\w+) => \w+, (\w+), (\d+), (\w+);", body):
                found[name] = kernel
        return found

    def syscfg_enable(self):
        m = re.search(r"SYSCFG_ENABLE: RccBit = RccBit::new\(Bus::(\w+), (\d+)\)", self.text)
        if not m:
            self.problems.append("no SYSCFG_ENABLE found")
            return None
        return m.group(1), int(m.group(2))

    def watchdog(self):
        """The token `Watchdog` takes and the registers it uses."""
        token = re.search(r"pub type WatchdogPeripheral = (\w+);", self.text)
        regs = re.search(r"fn iwdg_regs\(\) -> pac::iwdg::Iwdg \{\s*pac::(\w+)", self.text)
        if not token or not regs:
            self.problems.append("no watchdog peripheral found")
            return None
        return token.group(1), regs.group(1)

    def stream_vectors(self):
        """{(controller, stream): vector}."""
        found = {}
        for body in self.rows("stream_interrupts"):
            for controller, streams in re.findall(r"(\d+): \[(.*?)\];", body, re.S):
                for stream, vector in re.findall(r"(\d+) => (\w+)", streams):
                    found[(int(controller), int(stream))] = vector
        return found

    def sdmmcs(self):
        found = {}
        if not re.search(r"^sdmmcs!\(", self.text, re.M):
            return found
        for body in self.rows("sdmmcs"):
            for name, bus, bit in re.findall(r"(\w+) => (\w+), (\d+);", body):
                found[name] = (bus, int(bit))
        return found

    def adcs(self):
        found = {}
        for body in self.rows("adcs"):
            for name, bus, bit in re.findall(r"(\w+) => \w+, (\w+), (\d+), \w+;", body):
                found[name] = (bus, int(bit))
        return found

    def adc_pins(self):
        found = {}
        for name, body in re.findall(r"^adc_pins!\((\w+):(.*?)\);", self.text, re.S | re.M):
            for port, pin, channel in re.findall(r"\('(\w)', (\d+), (\d+)\)", body):
                found.setdefault(name, set()).add((f"P{port}{pin}", int(channel)))
        return found

    def adc_dma(self):
        """{name: {(controller, stream, request)}} and {name: mux request}."""
        fixed, mux = {}, {}
        for name, body in re.findall(r"^adc_dma!\((\w+):(.*?)\);", self.text, re.S | re.M):
            for row in re.findall(r"\((\d+), (\d+), (\d+)\)", body):
                fixed.setdefault(name, set()).add(tuple(int(x) for x in row))
        for name, request in re.findall(r"^adc_dma_mux!\((\w+): mux (\d+)\);", self.text, re.M):
            mux[name] = int(request)
        return fixed, mux

    def timers(self):
        found = {}
        if not re.search(r"^timers!\(", self.text, re.M):
            return found
        for body in self.rows("timers"):
            for name, bus, bit, update, bits in re.findall(r"(\w+) => (\w+), (\d+), (\w+), (\d+);", body):
                found[name] = ((bus, int(bit)), update, int(bits))
        return found

    def channel_timers(self):
        found = {}
        if not re.search(r"^channel_timers!\(", self.text, re.M):
            return found
        for body in self.rows("channel_timers"):
            for name, channels, kind in re.findall(r"(\w+) => (\d+), (plain|break);", body):
                found[name] = (int(channels), kind == "break")
        return found

    def usb_fs(self):
        m = re.search(r"^usb_fs!\(\s*(\w+) => (\w+), (\d+), (\w+), \d+, \d+, \w+;", self.text, re.M)
        return (m.group(1), (m.group(2), int(m.group(3))), m.group(4)) if m else None

    def timer_pins(self):
        found = {}
        for tim, signal, rows in re.findall(r"^timer_pins!\((\w+) (\w+): (.*?)\);", self.text, re.M):
            for port, pin, af in re.findall(r"\('(\w)', (\d+), (\d+)\)", rows):
                found.setdefault(tim, set()).add((signal, f"P{port}{pin}", int(af)))
        return found

    def timer_dma(self):
        found = {}
        for tim, signal, rows in re.findall(r"^timer_dma!\((\w+) (\w+): (.*?)\);", self.text, re.M):
            mux = re.fullmatch(r"mux (\d+)", rows.strip())
            if mux:
                found.setdefault(tim, set()).add((signal, int(mux.group(1))))
            for dma, stream, request in re.findall(r"\((\d+), (\d+), (\d+)\)", rows):
                found.setdefault(tim, set()).add((signal, f"DMA{dma}_CH{stream}", int(request)))
        return found

    def pins(self):
        found = {}
        for body in self.rows("pins"):
            m = re.fullmatch(r"(Tx|Rx|Sck|Miso|Mosi|Ck|Cmd|D0|D1|D2|D3)Pin<(\w+)>: (.*)", body.strip(), re.S)
            if not m:
                self.problems.append(f"cannot parse pins!({body})")
                continue
            signal, usart, rows = m.groups()
            for port, pin, af in re.findall(r"\('(\w)', (\d+), (\d+)\)", rows):
                found.setdefault(usart, set()).add((signal.upper(), f"P{port}{pin}", int(af)))
        return found

    def fixed_dma(self):
        found = {}
        for body in self.rows("dma"):
            m = re.fullmatch(r"(?:Spi)?(Tx|Rx)Dma<(\w+)>: (.*)", body.strip(), re.S)
            if not m:
                return None
            signal, usart, rows = m.groups()
            for dma, stream, request in re.findall(r"\((\d+), (\d+), (\d+)\)", rows):
                found.setdefault(usart, set()).add(
                    (signal.upper(), f"DMA{dma}_CH{stream}", None, int(request))
                )
        return found

    def dmamux_dma(self):
        found = {}
        for body in self.rows("dma"):
            for usart, rx, tx in re.findall(r"(\w+): rx (\d+), tx (\d+);", body):
                found[usart] = {("RX", int(rx)), ("TX", int(tx))}
        return found

    def ports(self):
        bus = re.search(r"impl PortInstance for Port<\$letter>.*?RccBit::new\(Bus::(\w+), \$bit\)", self.text, re.S)
        found = {}
        for body in self.rows("ports"):
            for letter, regs, bit in re.findall(r"'(\w)' => (\w+), (\d+);", body):
                found[regs] = (bus.group(1), int(bit))
        return found

    def dma_controllers(self):
        found = {}
        for n, bus, bit in re.findall(
            r"impl DmaInstance for Dma<(\d)> \{.*?RccBit::new\(Bus::(\w+), (\d+)\)", self.text, re.S
        ):
            found[f"DMA{n}"] = (bus, int(bit))
        return found

    def mux_bases(self):
        return {
            f"DMA{n}": int(base)
            for n, base in re.findall(r"impl DmaInstance for Dma<(\d)> \{.*?const MUX_BASE: u8 = (\d+);", self.text, re.S)
        }


def exti_vectors():
    """{line: vector} as `vector` in src/exti.rs gives it, on every chip."""
    body = re.search(r"pub const fn vector\(n: u8\) -> Interrupt \{(.*?)\n\}", (ROOT / "src" / "exti.rs").read_text(), re.S).group(1)
    found = {}
    for arm, vector in re.findall(r"(\d+(?:\.\.=\d+)?|_) => Interrupt::(\w+)", body):
        if arm == "_":
            lines = [n for n in range(16) if n not in found]
        elif ".." in arm:
            low, high = arm.split("..=")
            lines = range(int(low), int(high) + 1)
        else:
            lines = [int(arm)]
        for line in lines:
            found[line] = vector
    return found


def check(chip, glue_path, metapac):
    meta = Metadata(metapac, chip)
    glue = Glue(glue_path)
    failures, checked = [], 0

    def compare(what, ours, theirs):
        nonlocal checked
        checked += 1
        if ours != theirs:
            failures.append(f"{what}: glue {sorted(ours) if isinstance(ours, set) else ours}, metadata {sorted(theirs) if isinstance(theirs, set) else theirs}")

    usarts = glue.usarts()
    spis = glue.spis()
    pins = glue.pins()
    fixed = glue.fixed_dma()
    mux = glue.dmamux_dma() if fixed is None else None
    instances = [(name, enable, ("TX", "RX")) for name, enable in sorted(usarts.items())]
    instances += [(name, enable, ("SCK", "MISO", "MOSI")) for name, enable in sorted(spis.items())]
    for name, enable, signals in instances:
        if meta.peripheral(name) is None:
            failures.append(f"{name}: not in the metadata")
            continue
        compare(f"{name} RCC enable", enable, meta.enable(name))
        theirs = {p for p in meta.pins(name) if p[0] in signals}
        compare(f"{name} pins", pins.get(name, set()), theirs)
        routes = {r for r in meta.dma(name) if r[0] in ("TX", "RX")}
        if fixed is not None:
            compare(f"{name} DMA routes", fixed.get(name, set()), routes)
        else:
            theirs = {(signal, request) for signal, _, dmamux, request in routes if dmamux == "DMAMUX1"}
            compare(f"{name} DMAMUX1 requests", mux.get(name, set()), theirs)
    timers = glue.timers()
    for name, (enable, update, bits) in sorted(timers.items()):
        if meta.peripheral(name) is None:
            failures.append(f"{name}: not in the metadata")
            continue
        compare(f"{name} RCC enable", enable, meta.enable(name))
        compare(f"{name} update vector", update, meta.interrupt(name, "UP"))
        compare(f"{name} counter bits", bits, 32 if meta.block(name) == "TIM_GP32" else 16)
    blocks = {"TIM_ADV": (4, True), "TIM_GP16": (4, False), "TIM_GP32": (4, False), "TIM_2CH": (2, False),
              "TIM_1CH": (1, False), "TIM_2CH_CMP": (2, True), "TIM_1CH_CMP": (1, True)}
    channel_timers = glue.channel_timers()
    timer_pins = glue.timer_pins()
    timer_dma = glue.timer_dma()
    for name in sorted(n for n in timers if blocks.get(meta.block(n))):
        compare(f"{name} channels and break unit", channel_timers.get(name), blocks[meta.block(name)])
        theirs = {p for p in meta.pins(name) if re.fullmatch(r"CH[1-4]N?", p[0])}
        compare(f"{name} channel pins", timer_pins.get(name, set()), theirs)
        routes = {r for r in meta.dma(name) if re.fullmatch(r"CH[1-4]|UP", r[0])}
        ours = timer_dma.get(name, set())
        if fixed is not None:
            pairs = {(signal, channel) for signal, channel, _, _ in routes}
            compare(f"{name} DMA (signal, stream) pairs", {(s_, c) for s_, c, _ in ours}, pairs)
            wrong = {r for r in ours if (r[0], r[1], None, r[2]) not in routes}
            compare(f"{name} DMA requests are the metadata's", wrong, set())
        else:
            theirs = {(signal, request) for signal, _, dmamux, request in routes if dmamux == "DMAMUX1"}
            compare(f"{name} DMAMUX1 requests", ours, theirs)
    usb = glue.usb_fs()
    if usb:
        name, enable, vector = usb
        compare(f"{name} RCC enable", enable, meta.enable(name))
        vectors = set(re.findall(r'interrupt: "(\w+)"', meta.peripheral(name)))
        compare(f"{name} vector is the metadata's", vector in vectors, True)
        theirs = {p for p in meta.pins(name) if p[0] in ("DM", "DP")}
        compare(f"{name} pins", {("DM", "PA11", 10), ("DP", "PA12", 10)}, theirs)
    for regs, enable in sorted(glue.ports().items()):
        compare(f"{regs} RCC enable", enable, meta.enable(regs))
    for dma, enable in sorted(glue.dma_controllers().items()):
        compare(f"{dma} RCC enable", enable, meta.enable(dma))
    for dma, base in sorted(glue.mux_bases().items()):
        compare(f"{dma} first DMAMUX channel", base, meta.dmamux_channel(f"{dma}_CH0"))
    for name, kernel in sorted(glue.spi_kernels().items()):
        compare(f"{name} kernel clock", kernel, SPI_KERNEL.get(meta.kernel_clock(name)))
    syscfg = glue.syscfg_enable()
    if syscfg:
        compare("SYSCFG RCC enable", syscfg, meta.enable("SYSCFG"))
    watchdog = glue.watchdog()
    if watchdog:
        token, regs = watchdog
        compare("watchdog token and registers are one peripheral", token, regs)
        compare(f"{token} is a watchdog", token in meta.names("iwdg"), True)
    for name, enable in sorted(glue.sdmmcs().items()):
        compare(f"{name} RCC enable", enable, meta.enable(name))
        signals = ("CK", "CMD", "D0", "D1", "D2", "D3")
        theirs = {p for p in meta.pins(name) if p[0] in signals}
        compare(f"{name} pins", pins.get(name, set()), theirs)
    adc_fixed, adc_mux = glue.adc_dma()
    adc_pins = glue.adc_pins()
    for name, enable in sorted(glue.adcs().items()):
        compare(f"{name} RCC enable", enable, meta.enable(name))
        theirs = {
            (pin, int(re.search(r"\d+", signal).group()))
            for signal, pin, _ in meta.analog_pins(name)
            if re.fullmatch(r"IN(P)?\d+", signal) and not pin.endswith("_C")
        }
        compare(f"{name} pins", adc_pins.get(name, set()), theirs)
        routes = meta.dma(name)
        if name in adc_mux:
            theirs = {request for _, _, dmamux, request in routes if dmamux == "DMAMUX1"}
            compare(f"{name} DMAMUX1 request", {adc_mux[name]}, theirs)
        else:
            theirs = {(int(ch[3]), int(ch[7:]), request) for _, ch, _, request in routes}
            compare(f"{name} DMA routes", adc_fixed.get(name, set()), theirs)
    streams = glue.stream_vectors()
    for (controller, stream), vector in sorted(streams.items()):
        compare(f"DMA{controller} stream {stream} vector", vector, meta.interrupt(f"DMA{controller}", f"CH{stream}"))
    compare("every DMA stream has a vector", len(streams), 16)
    for line, vector in sorted(exti_vectors().items()):
        compare(f"EXTI line {line} vector", vector, meta.interrupt("EXTI", f"EXTI{line}"))

    failures += glue.problems
    left_out = [n for n in meta.names("usart") if n not in usarts]
    left_out += [n for n in meta.names("spi") if n not in spis]
    left_out += [n for n in meta.names("timer") if n not in timers]
    print(f"{chip}: {checked} facts checked against stm32-metapac metadata", end="")
    print(f"; not routed in the glue: {', '.join(left_out)}" if left_out else "")
    for failure in failures:
        print(f"  MISMATCH {failure}")
    return not failures


def main():
    args = sys.argv[1:]
    glue_override = None
    if "--glue" in args:
        i = args.index("--glue")
        glue_override = Path(args[i + 1])
        del args[i : i + 2]
    chips = args or sorted(p.stem for p in (ROOT / "src" / "chips").glob("*.rs"))
    metapac = metapac_dir()
    print(f"stm32-metapac at {metapac}")
    ok = True
    for chip in chips:
        ok &= check(chip, glue_override or ROOT / "src" / "chips" / f"{chip}.rs", metapac)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()

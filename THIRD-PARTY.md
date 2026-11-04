# Third-party code

FerroHAL is licensed under MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`). Parts of it are ported from the projects below, under their licences, which are the same.

## embassy-stm32

- **Source:** embassy-stm32 0.6.0, <https://github.com/embassy-rs/embassy>
- **Licence:** MIT OR Apache-2.0
- **Copyright:** Embassy project contributors

What was ported:

| FerroHAL | From embassy-stm32 | What |
|---|---|---|
| `src/rcc_h7rm0433/mod.rs` | `src/rcc/h.rs` | The power-up order (PWR supply configuration, ACTVOSRDY, voltage scale, HSI, HSE, PLL, flash wait states, prescalers, switch); the PLL register programming (DIVM, PLLRGE, VCO range selection, DIV fields holding the divider less one); the voltage-scale clock limits; the RM0433 flash wait-state table, also checked against RM0433 table 17 |
| `src/rcc_f4/mod.rs` | `src/rcc/f247.rs` | The F401 clock limits (SYSCLK, APB, HSE and PLL input ranges) and the flash wait-state rule (one per 30 MHz at 2.7 to 3.6 V) |

The overall structure is also embassy's design, though not its code: one module per stm32-metapac IP version, and instance traits that return an erased static `Info`.

The F4 clock code deliberately departs from embassy in one place: the voltage scale. Embassy's `f247.rs` writes VOS Scale 1 on every F4, which is reserved on the F401 (see `ferrohal-architecture.md` §6). FerroHAL writes the scale each chip file names, from RM0368 and RM0090, with the PLL off, and waits for VOSRDY before switching to the PLL.

Embassy's MIT notice:

```text
Copyright (c) Embassy project contributors

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
```

## Data sources, not code

- **Routing tables:** the glue's routing facts in `src/chips/*.rs` (pins, alternate functions, DMA requests, RCC bits) come from stm32-metapac 21.0.0's metadata (MIT OR Apache-2.0). `tools/check_glue.py` checks them against it.
- **Limits and sequences:** clock limits, register sequences and errata workarounds come from ST's reference manuals, datasheets and errata sheets, which are cited where they're used.

# `free()`: the landed shapes, driver by driver

What blocked each driver's teardown, and the shape that landed. The rules these
shapes meet are in [`free.md`](free.md).

Source: ADR-002, [`../decisions/2026-11-08-free-teardown-brief.md`](../decisions/2026-11-08-free-teardown-brief.md).

- ~~`Pwm<T>` held only `PhantomData<T>`~~ **done (2026-11-10):** it stores the token;
  `free()` stops the counter, closes the compare requests and hands it back.
- ~~`PwmChannel`/`CompareLane` held register blocks, so a lane could drive a freed
  timer~~ **done (2026-11-10):** `PwmChannel<'a, T, C>` borrows the handle, so it
  cannot outlive `free()`; `CompareLane<T, C>` holds no timer access — its compare
  writes take a `&Pwm` per call — so it stays movable into an `Awg` beside the timer
  it drives. A lane's stream-error path stops the stream only; the firmware parks the
  output through the handle.
- ~~`Tick`/`Timebase` were not generic~~ **done (2026-11-10):** both carry their token
  and hand it back with `free()`. The bench seals a running tick in a per-run enum
  (`hil/common/timers.rs`); `Link` reads the timebase through a `TimebaseReader`.
- ~~`Uart`/`UartRx` stored `&'static Info`, and the halves needed a rejoin~~
  **done (2026-11-11):** the token rides in `Line<U>`; `split()`/`join()`; `free()` —
  the whole `Uart`, or `UartRx::free(self, line)` — returns it.
- ~~The UART halves were instance-typed by construction only, and a receive-only
  `UartRx::free` took any `Line`~~ **done (2026-11-13):** `UartTx<U>`/`UartRx<U>`
  carry the instance, so another UART's halves do not compile into `join`,
  `set_config`, `send_break` or `free`; `UartRx::new` returns an `RxLine<U>` (mask and
  unmask only), and `UartRx::free` takes only that, so a split UART's receive half
  cannot free the line under a live `UartTx`. `UartIrq` stays untyped: it is inert
  after a free. The bench's `dyn Link` reaches the halves through `TxHalf`/`RxHalf`
  (`hil/common/tests.rs`).
- ~~An `Awg` kept its `Pwm`s and lanes for good~~ **done (2026-11-13):**
  `Awg::free(self) -> (A, Option<B>)` runs `abort` and frees each `Pwm`; the lanes
  end with it. Testing it found a stopped timer holds each output at the level it
  stopped at, since the duties are preloaded: `abort` and `Pwm::free` now latch
  them (an update event), so the lines go low at once.
- ~~`Spi`/`I2c` stored `&'static Info`~~ **done (2026-11-12):** `Spi<U>`,
  `BlockingSpi<U>` and `I2c<U>` carry the token; `free()` stops the streams (SPI) or
  closes the unit (I2C, polled SPI) and returns it, and a refusal hands the token back
  with the wiring. `Spi::check` reads the instance from the type.
- ~~UART, SPI, I2C and the timer lanes erased their streams into a `Channel` and freed
  only the instance token, stranding pins and streams~~ **done (2026-11-13, plan
  step 3):** each handle is generic over its wiring (`Uart<W>`, `Spi<W>`, `I2c<W>`,
  `CompareLane<T, C, W>`) through a sealed `UartParts`/`SpiParts`/`I2cParts` trait, and
  `free()` rebuilds the wiring it was opened from, typed pins and stream tokens. A lane
  is assembled from its wiring (`LaneWiring::assemble`), and its `free(&Pwm)` returns
  the wiring and buffers; a `PwmChannel` keeps its pin and closes its output on `free`.
  Lanes still name their channel as a const generic beside the wiring: stable Rust
  cannot derive it from the pin's type.

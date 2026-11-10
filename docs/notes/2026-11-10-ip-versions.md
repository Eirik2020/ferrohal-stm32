# IP versions across families (stm32-metapac 21, measured)

From the architecture map's section 11, dissolved 2026-11-10. Its decided scope
items are superseded by [ADR-005](../decisions/2026-10-31-ferrowasp-integration.md),
and its two open questions - the prototype's kill condition, and how to measure
reuse - closed when FerroWasp adopted FerroHAL ([RESOLVED](../../RESOLVED.md),
2026-11-10).

Constraint *(measured, stm32-metapac 21)*: F4 and G4 share almost no IP versions,
so a G4 UART run measures what a second family costs from scratch, not reuse.
Reuse can be measured on GPIO and EXTI between F4 and G4 now, or on UART between
G4, H5 and H7 later.

| Peripheral | F401 / F405 | G431 / G474 | F722 | H743 | H563 |
|---|---|---|---|---|---|
| USART | usart_v2 | usart_v4 | usart_v3 | usart_v4 | usart_v4 |
| DMA | dma_v2 | bdma_v1 + dmamux_v1 | dma_v2 | dma_v1 + bdma_v1 + dmamux_v1 | gpdma_v1 |
| Timer | timer_v1 | timer_v2 | timer_v1 | timer_v3 | timer_v2 |
| SPI | spi_v2_i2s | spi_v3_i2s | spi_v3_i2s | spi_v4_i2s | spi_v5_i2s |
| GPIO | gpio_v2 | gpio_v2 | gpio_v2 | gpio_v2 | gpio_v2 |
| EXTI | exti_v1 | exti_v1 | exti_v1 | exti_h7 | exti_h5 |
| RCC / PWR / FLASH | rcc_f4 / pwr_f4 / flash_f4 | rcc_g4 / pwr_g4 / flash_g4c* | – | – | – |

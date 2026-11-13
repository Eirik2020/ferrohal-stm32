# The requirements and verification model

Source: ADR-006, [`../decisions/2026-11-10-doc-map-dissolution.md`](../decisions/2026-11-10-doc-map-dissolution.md).


The architecture map's section 9's model, dissolved 2026-11-10; decided by the
owner on 2026-11-03. The catalog half is [requirements-ids.md](requirements-ids.md);
the format and reqtool are [requirements-tooling.md](requirements-tooling.md).

## The general rule

- **[Decided]** A car consists of an engine, wheels, a chassis and suspension. The car has its own requirements and sets requirements on its components. A first pass verifies that the car works as required. Later a set of wheels can be replaced by another that fulfils the wheel requirements, without reverifying the car.
- **[Decided]** A layer's interface to the layer below is the requirements it sets there. A change to a part that still fulfils them re-owes only that part. A change to the requirements re-owes the layer that set them.
- **[Decided]** The layers, each with its own requirements and verification: IP block, engine, chip glue, HAL API.
- **[Decided]** One first pass tests everything. After it, only what changed is retested: an IP change retests the IP, a glue change retests the glue.
- **[Decided 2026-11-13]** The car rule across chips. IP versions are separate from chip families, so an IP version proven on one chip is proven for every family that uses it, and glue a family shares is proven on one chip of that family. Only glue particular to one chip needs that chip. A second chip proving the same layer is exhaustive testing, which is forbidden unless a rationale is presented to the owner and accepted.
- **[Superseded 2026-11-13, ADR-009]** ~~The flight-controller benches (Foxeer F405 V2, TBS Lucid H7) are smoke tests: they sanity-check the F405 and the H743 and carry no requirement's evidence. Requirements are proven on the Nucleos, one chip per IP version or family glue.~~ A run is evidence at its bench's tier (`HIL`, `DBG`, `FLSH`, ...); the flashed boards' runs are `FLSH` evidence, and evidence has three axes, IP version, die and series ([maturity.md](maturity.md)).
- **[Decided 2026-11-13]** Evidence follows files, not images. A requirement verified on a bench stands while the files that bench exercised for it are unchanged; another file added to the image, another run's code, or a rebuilt binary does not re-owe it. reqtool does this since reqtool#4 and #5 (2026-11-13): board evidence holds while its declaring files, and the external crates they import, are unchanged; CI pins that revision (`.github/workflows/ci.yml`).

## Files and hashes

- **[Decided]** Things are grouped into files, for example `usart_v2`. The file states its requirements and is hashed. Verification evidence records the hash, and evidence that does not match the hash is flagged as unverified.
- **[Decided]** Another local file is not part of a file's hash; the requirements cover that link. A consequence: a file serving two IP versions, such as `dma_stream.rs`, has one hash, so changing it re-owes both unless it is split.
- **[Proposal]** The tool keeps two labels for the flag: `stale` when a pass exists and the hash has changed, `unverified` when none ever passed. Both count as not verified and hold a gate closed.
- **[Proposal]** A file's evidence also covers the `macro_rules!` definitions it expands, since a macro changes what a file compiles to without changing its hash. `chips/shared/macros.rs` expands in all four chip files. Requirement docs and markers for internal macros go inside the macro body, so each expansion carries them.

## Dependencies

- **[Decided]** Dependencies are pinned. Bumping one breaks the verification of what uses it, and requires a diff analysis or reverification.
- **[Decided]** Contributors may do a diff analysis. A maintainer must review it. The owner is the only maintainer for now.
- **[Decided]** The compiler does not count as a pinned dependency. Differences between compiler versions are negligible outside safety assurance, and neither FerroHAL nor FerroWasp counts as safety assurance.

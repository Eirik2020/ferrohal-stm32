//! Silicon errata as data (ADR-010, `docs/design/errata.md`): every erratum of
//! the die's errata sheet, the revisions it affects, and how FerroHAL handles
//! it. The chip's table is [`chip::ERRATA`](crate::chip::ERRATA), from
//! [`chip::ERRATA_SHEET`](crate::chip::ERRATA_SHEET).
//!
//! A workaround checks its erratum with [`Erratum::affects`], never a raw
//! revision code.
//!
//! Requirements: GLUE-007, GLUE-008, GLUE-009

/// An ST errata sheet, as transcribed.
pub struct Sheet {
    /// `ES0392`.
    pub name: &'static str,
    /// The sheet's own revision the entries were read from: section numbers
    /// move between revisions.
    pub revision: u8,
    /// The die's revision codes (`DBGMCU_IDCODE.REV_ID`), oldest first, with
    /// the markings each covers.
    pub revisions: &'static [(u16, &'static str)],
}

/// The sheet's own status for an erratum on the revisions it affects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SheetStatus {
    /// A workaround is available (`A`).
    Workaround,
    /// A partial workaround (`P`).
    Partial,
    /// No workaround (`N`).
    None,
}

/// How FerroHAL handles an erratum. The text says where or what.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handling {
    /// FerroHAL applies a workaround on affected revisions.
    Workaround(&'static str),
    /// FerroHAL's code never meets the condition.
    Avoided(&'static str),
    /// A rule the firmware follows.
    Firmware(&'static str),
    /// A package or board fact, with no firmware action.
    Board(&'static str),
    /// No FerroHAL path reaches it.
    NotReached(&'static str),
    /// Affects FerroHAL and is not handled yet: the DEBT.md item's title.
    Open(&'static str),
    /// Transcribed from the sheet, not yet reviewed.
    Unreviewed,
}

/// One erratum of a sheet.
pub struct Erratum {
    pub sheet: &'static Sheet,
    /// `2.2.10`, as numbered in the sheet's revision.
    pub section: &'static str,
    pub title: &'static str,
    /// The revision codes the sheet marks affected.
    pub affected: &'static [u16],
    pub status: SheetStatus,
    pub handling: Handling,
    /// The requirement IDs that carry it, for reqtool.
    pub requirements: &'static [&'static str],
}

impl Erratum {
    /// Whether the erratum applies to the die revision `rev`. A revision the
    /// sheet lists is affected when marked. One newer than the sheet's newest
    /// takes the newest's verdict, so an erratum unfixed there stays applied;
    /// any other revision the sheet does not know is affected.
    pub const fn affects(&self, rev: u16) -> bool {
        let known = self.sheet.revisions;
        let mut i = 0;
        let mut newest = 0;
        while i < known.len() {
            if known[i].0 == rev {
                return listed(self.affected, rev);
            }
            if known[i].0 > newest {
                newest = known[i].0;
            }
            i += 1;
        }
        if rev > newest && !known.is_empty() {
            return listed(self.affected, newest);
        }
        true
    }

    /// `ES0392 2.2.10`.
    pub const fn id(&self) -> (&'static str, &'static str) {
        (self.sheet.name, self.section)
    }
}

const fn listed(codes: &[u16], rev: u16) -> bool {
    let mut i = 0;
    while i < codes.len() {
        if codes[i] == rev {
            return true;
        }
        i += 1;
    }
    false
}

/// The table `docs/errata/<sheet>.md` holds between its generated markers:
/// one row per erratum. Host-side, for the documentation check.
#[cfg(test)]
pub(crate) fn render(sheet: &Sheet, errata: &[Erratum]) -> std::string::String {
    use std::fmt::Write;
    let marking = |code: u16| {
        sheet
            .revisions
            .iter()
            .find(|(c, _)| *c == code)
            .map_or("?", |(_, m)| *m)
    };
    let mut out = std::format!(
        "{} Rev {}. Revisions: {}.\n\n| § | erratum | affected | sheet | FerroHAL | requirements |\n|---|---|---|---|---|---|\n",
        sheet.name,
        sheet.revision,
        sheet
            .revisions
            .iter()
            .map(|(code, m)| std::format!("{m} ({code:#06x})"))
            .collect::<std::vec::Vec<_>>()
            .join(", ")
    );
    for e in errata {
        let affected = if e.affected.len() == sheet.revisions.len() {
            "all".into()
        } else {
            e.affected
                .iter()
                .map(|c| marking(*c))
                .collect::<std::vec::Vec<_>>()
                .join(", ")
        };
        let status = match e.status {
            SheetStatus::Workaround => "workaround",
            SheetStatus::Partial => "partial",
            SheetStatus::None => "none",
        };
        let handling = match e.handling {
            Handling::Workaround(t) => std::format!("**workaround**: {t}"),
            Handling::Avoided(t) => std::format!("avoided: {t}"),
            Handling::Firmware(t) => std::format!("firmware: {t}"),
            Handling::Board(t) => std::format!("board: {t}"),
            Handling::NotReached(t) => std::format!("not reached: {t}"),
            Handling::Open(t) => std::format!("**open**: DEBT.md, {t}"),
            Handling::Unreviewed => "unreviewed".into(),
        };
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} |",
            e.section,
            e.title,
            affected,
            status,
            handling,
            e.requirements.join(", ")
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chip::{ERRATA, ERRATA_SHEET};

    const SHEET: Sheet = Sheet {
        name: "ES0000",
        revision: 1,
        revisions: &[(0x1000, "A"), (0x1001, "Z"), (0x1003, "1")],
    };

    const fn erratum(affected: &'static [u16]) -> Erratum {
        Erratum {
            sheet: &SHEET,
            section: "2.1.1",
            title: "t",
            affected,
            status: SheetStatus::Workaround,
            handling: Handling::Unreviewed,
            requirements: &[],
        }
    }

    #[test]
    fn a_known_revision_is_affected_when_listed() {
        let fixed_after_a = erratum(&[0x1000]);
        assert!(fixed_after_a.affects(0x1000));
        assert!(!fixed_after_a.affects(0x1001));
        assert!(!fixed_after_a.affects(0x1003));
    }

    #[test]
    fn a_newer_revision_takes_the_newest_verdict() {
        assert!(!erratum(&[0x1000]).affects(0x2000));
        assert!(erratum(&[0x1000, 0x1001, 0x1003]).affects(0x2000));
    }

    #[test]
    fn another_unknown_revision_is_affected() {
        assert!(erratum(&[]).affects(0x1002));
        assert!(erratum(&[]).affects(0x0fff));
    }

    /// Every code the chip's table marks affected is one of its sheet's
    /// revisions.
    #[test]
    fn every_affected_code_is_a_sheet_revision() {
        for erratum in ERRATA {
            for code in erratum.affected {
                assert!(
                    ERRATA_SHEET
                        .revisions
                        .iter()
                        .any(|(known, _)| known == code),
                    "{} {}: {code:#06x} is not one of the sheet's revisions",
                    ERRATA_SHEET.name,
                    erratum.section
                );
            }
        }
    }

    /// The section numbers of `sheet` cited in `text`: `ES0392 2.2.10`.
    fn citations<'a>(text: &'a str, sheet: &str) -> std::vec::Vec<&'a str> {
        let mut out = std::vec::Vec::new();
        let needle = std::format!("{sheet} ");
        let mut rest = text;
        while let Some(at) = rest.find(needle.as_str()) {
            rest = &rest[at + needle.len()..];
            let end = rest
                .find(|c: char| !(c.is_ascii_digit() || c == '.'))
                .unwrap_or(rest.len());
            let section = rest[..end].trim_end_matches('.');
            if section.matches('.').count() >= 1
                && section.starts_with(|c: char| c.is_ascii_digit())
            {
                out.push(section);
            }
        }
        out
    }

    fn sources(dir: &std::path::Path, out: &mut std::vec::Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                sources(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    /// The chip's table is its documentation, and the source cites only
    /// reviewed entries and every workaround (ADR-010).
    /// `FERROHAL_BLESS_ERRATA=1` rewrites `docs/errata/<sheet>.md`.
    #[test]
    fn the_chip_table_matches_its_documentation_and_the_source() {
        use std::{string::String, vec::Vec};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let sheet = &ERRATA_SHEET;
        let errata = ERRATA;
        let mut problems: Vec<String> = Vec::new();

        let path = root.join(std::format!("docs/errata/{}.md", sheet.name));
        let table = render(sheet, errata);
        let doc = std::fs::read_to_string(&path).unwrap_or_default();
        let (open, close) = ("<!-- generated: errata -->\n", "<!-- end generated -->");
        match (doc.find(open), doc.find(close)) {
            (Some(start), Some(end)) if start < end => {
                if doc[start + open.len()..end] != table {
                    if std::env::var_os("FERROHAL_BLESS_ERRATA").is_some() {
                        let fresh =
                            std::format!("{}{table}{}", &doc[..start + open.len()], &doc[end..]);
                        std::fs::write(&path, fresh).unwrap();
                    } else {
                        problems.push(std::format!(
                            "{} differs from the table; FERROHAL_BLESS_ERRATA=1 rewrites it",
                            path.display()
                        ));
                    }
                }
            }
            _ => problems.push(std::format!("{}: no generated markers", path.display())),
        }

        let mut files = Vec::new();
        sources(&root.join("src"), &mut files);
        let mut cited = std::collections::BTreeSet::new();
        for file in &files {
            if file
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("errata_"))
            {
                continue;
            }
            let text = std::fs::read_to_string(file).unwrap_or_default();
            for section in citations(&text, sheet.name) {
                cited.insert(String::from(section));
                match errata.iter().find(|e| e.section == section) {
                    None => problems.push(std::format!(
                        "{} cites {} {section}, which has no entry",
                        file.display(),
                        sheet.name
                    )),
                    Some(e) if e.handling == Handling::Unreviewed => problems.push(std::format!(
                        "{} cites {} {section}, whose entry is unreviewed",
                        file.display(),
                        sheet.name
                    )),
                    Some(_) => {}
                }
            }
        }
        let debt = std::fs::read_to_string(root.join("docs/DEBT.md")).unwrap_or_default();
        for e in errata {
            match e.handling {
                Handling::Workaround(_) if !cited.contains(e.section) => {
                    problems.push(std::format!(
                        "{} {} is a workaround no source file cites",
                        sheet.name,
                        e.section
                    ))
                }
                Handling::Open(item) if !debt.contains(item) => problems.push(std::format!(
                    "{} {} names DEBT item \"{item}\", which DEBT.md does not hold",
                    sheet.name,
                    e.section
                )),
                _ => {}
            }
        }
        assert!(problems.is_empty(), "{problems:#?}");
    }
}

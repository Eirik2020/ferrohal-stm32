"""Every public function in src/ is called by checks/no-panic, or listed.

    python3 -I tools/check_no_panic_coverage.py

checks/no-panic proves no panic is reachable through the calls it makes, and
only those (docs/DEBT.md had it as "proves only the paths it calls"). This
check closes the gap the stable way: it lists every `pub fn`
under src/ and requires checks/no-panic/src/main.rs to name it, or
checks/no-panic/uncalled.txt to list it with a reason a reviewer accepted.

How a function counts as called, from the source text alone:

- an associated function without `self` (`Uart::new`) by its type's path,
  `Type::name` or `Type::<..>::name`;
- a method, or a free function, by its name anywhere in the program.

The second is an approximation: two methods sharing a name count together. The
exact list would come from rustdoc's JSON, which needs a nightly toolchain; the
owner chose this one. Items under `#[cfg(test)]` are not API and are skipped,
and so is a file whose module is private (`mod x;`, `pub(crate) mod x;`) along
with its parents' chain, unless the parent re-exports the type or function
with `pub use`, or any file does with `pub use crate::<its path>::`.

Fails on a function neither called nor listed, and on a listed one that is now
called or no longer exists, so the list cannot go stale.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PROGRAM = ROOT / "checks/no-panic/src/main.rs"
LISTED = ROOT / "checks/no-panic/uncalled.txt"

STRING = re.compile(r'b?"(?:\\.|[^"\\])*"|b?\'(?:\\.|[^\\\'])\'')
COMMENT = re.compile(r"//[^\n]*")
BLOCK_COMMENT = re.compile(r"/\*.*?\*/", re.S)


def clean(text):
    """The source with comments and literals blanked, line breaks kept."""
    text = BLOCK_COMMENT.sub(lambda m: "\n" * m.group(0).count("\n"), text)
    text = STRING.sub('""', text)
    return COMMENT.sub("", text)


def block_end(text, open_at):
    """The index just past the brace matching the one at `open_at`."""
    depth = 0
    for i in range(open_at, len(text)):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
            if depth == 0:
                return i + 1
    return len(text)


def drop_test_items(text):
    """Blanks every item under `#[cfg(test)]`."""
    while True:
        m = re.search(r"#\[cfg\(test\)\]", text)
        if not m:
            return text
        brace = text.find("{", m.end())
        semi = text.find(";", m.end())
        if brace < 0 or (0 <= semi < brace):
            end = semi + 1
        else:
            end = block_end(text, brace)
        text = text[: m.start()] + "\n" * text[m.start():end].count("\n") + text[end:]


IMPL = re.compile(r"\bimpl\b(?:\s*<(?:[^<>]|<[^<>]*>)*>)?\s+([^{;]*?)\s*(?:where\b[^{]*)?\{")
PUB_FN = re.compile(r"\bpub\s+(?:const\s+)?(?:unsafe\s+)?(?:extern\s+\"[^\"]*\"\s+)?fn\s+([A-Za-z_]\w*)\s*(?:<(?:[^<>]|<[^<>]*>)*>)?\s*\(([^)]*)")


def impl_type(header):
    """The type an inherent impl is for, or None for a trait impl."""
    if re.search(r"\bfor\b", header):
        return None
    path = re.sub(r"<.*", "", header).strip()
    return path.split("::")[-1] or None


def functions(path):
    """(type or None, name, takes self, line) for each public function."""
    text = drop_test_items(clean(path.read_text()))
    impls = []
    for m in IMPL.finditer(text):
        brace = m.end() - 1
        impls.append((brace, block_end(text, brace), impl_type(m.group(1)), m.group(1)))
    found = []
    for m in PUB_FN.finditer(text):
        # pub(crate), pub(super): the regex wants a space after `pub`, so they
        # never match.
        at = m.start()
        inside = [i for i in impls if i[0] < at < i[1]]
        owner = max(inside, key=lambda i: i[0]) if inside else None
        if owner and owner[2] is None:
            continue  # a trait impl's method: the trait's, not this crate's API
        takes_self = bool(re.match(r"\s*(?:&\s*(?:'\w+\s+)?)?(?:mut\s+)?self\b", m.group(2)))
        line = text.count("\n", 0, at) + 1
        found.append((owner[2] if owner else None, m.group(1), takes_self, line))
    return found


def declaration(path):
    """(the parent file, the module's name) that declares `path`, or None."""
    src = ROOT / "src"
    rel = path.relative_to(src)
    if rel.as_posix() == "lib.rs":
        return None
    name = rel.parent.name if rel.name == "mod.rs" else rel.stem
    folder = path.parent.parent if rel.name == "mod.rs" else path.parent
    for parent in (folder / "mod.rs", folder.with_suffix(".rs"), folder / "lib.rs"):
        if parent.exists() and parent != path:
            return parent, name
    return None


def exported(path):
    """None when every item of `path` is reachable from outside the crate, else
    the set of names its private module's parent re-exports (`pub use`)."""
    found = declaration(path)
    if not found:
        return None
    parent, name = found
    text = clean(parent.read_text())
    if not re.search(rf"\bmod\s+{name}\s*[;{{]", text):
        return None  # declared some other way (#[path]): take it as public
    outer = exported(parent)
    if re.search(rf"(?<![\w)])pub\s+mod\s+{name}\b", text):
        return outer
    names = set()
    for m in re.finditer(rf"\bpub\s+use\s+(?:self::)?{name}::(\{{[^}}]*\}}|\w+)", text):
        names |= set(re.findall(r"\w+", m.group(1)))
    if outer is not None:
        names &= outer
    rel = path.relative_to(ROOT / "src").with_suffix("")
    module = "::".join(rel.parent.parts if rel.name == "mod" else rel.parts)
    for other in (ROOT / "src").rglob("*.rs"):
        for m in re.finditer(rf"\bpub\s+use\s+crate::{module}::(\{{[^}}]*\}}|\w+)", clean(other.read_text())):
            names |= set(re.findall(r"\w+", m.group(1)))
    return names


def called(program, type_, name, takes_self):
    if type_ and not takes_self:
        return re.search(rf"\b{re.escape(type_)}(?:::<(?:[^<>]|<[^<>]*>)*>)?::{name}\b", program)
    return re.search(rf"(?<![\w$]){name}\b(?!\s*!)", program)


def key(path, type_, name):
    return f"{path.relative_to(ROOT).as_posix()} {type_ + '::' if type_ else ''}{name}"


def listed():
    out = {}
    for n, line in enumerate(LISTED.read_text().splitlines(), 1):
        if not line.strip() or line.startswith("#"):
            continue
        m = re.match(r"(\S+ \S+)\s+--\s+(.+)", line)
        if not m:
            sys.exit(f"{LISTED.relative_to(ROOT)}:{n}: want `<file> <Type::name> -- <reason>`")
        out[m.group(1)] = m.group(2)
    return out


def main():
    program = clean(PROGRAM.read_text())
    exceptions = listed()
    problems, total, seen = [], 0, set()
    for path in sorted((ROOT / "src").rglob("*.rs")):
        reach = exported(path)
        for type_, name, takes_self, line in functions(path):
            if reach is not None and (type_ or name) not in reach:
                continue  # crate-internal: a private module's, not re-exported
            total += 1
            k = key(path, type_, name)
            seen.add(k)
            hit = called(program, type_, name, takes_self)
            if hit and k in exceptions:
                problems.append(f"{k} is called now; drop it from uncalled.txt")
            elif not hit and k not in exceptions:
                problems.append(f"{path.relative_to(ROOT)}:{line}: {k.split(' ', 1)[1]} is neither called by checks/no-panic nor listed in uncalled.txt")
    for k in sorted(set(exceptions) - seen):
        problems.append(f"{k} is listed in uncalled.txt but is no public function")
    for problem in problems:
        print(problem)
    if problems:
        print(f"FAIL  {len(problems)} of {total} public functions")
        sys.exit(1)
    print(f"pass  {total} public functions: {total - len(exceptions)} called, {len(exceptions)} listed")


if __name__ == "__main__":
    main()

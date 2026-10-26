"""Convert the approved Canvas intro's SVG path data to native GPUI commands.

The source HTML is supplied separately; no browser or video is bundled.
"""
import argparse
import json
import re
from pathlib import Path


def commands(path):
    tokens = re.findall(r"[-+]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][-+]?\d+)?|[A-Za-z]", path)
    index = 0
    mode = None
    x = y = sx = sy = 0.0
    result = []
    counts = {"M": 2, "L": 2, "H": 1, "V": 1, "Q": 4, "C": 6, "A": 7}
    number = lambda value: repr(float(value))
    while index < len(tokens):
        if tokens[index].isalpha():
            mode = tokens[index]
            index += 1
            if mode == "Z":
                result.append("Command::Close")
                x, y = sx, sy
                mode = None
                continue
        if mode not in counts:
            raise ValueError(f"Unsupported SVG command: {mode}")
        size = counts[mode]
        values = list(map(float, tokens[index:index + size]))
        if len(values) != size:
            raise ValueError("Truncated SVG path")
        index += size
        if mode == "M":
            x, y = sx, sy = values
            result.append(f"Command::Move({number(x)}, {number(y)})")
            mode = "L"
        elif mode in ("L", "H", "V"):
            if mode == "H":
                x = values[0]
            elif mode == "V":
                y = values[0]
            else:
                x, y = values
            result.append(f"Command::Line({number(x)}, {number(y)})")
        elif mode in ("Q", "C"):
            x, y = values[-2:]
            kind = "Quad" if mode == "Q" else "Cubic"
            result.append(f"Command::{kind}({', '.join(map(number, values))})")
        else:
            x, y = values[-2:]
            rx, ry, angle, large, sweep, x, y = values
            result.append(f"Command::Arc({number(rx)}, {number(ry)}, {number(angle)}, "
                          f"{str(bool(large)).lower()}, {str(bool(sweep)).lower()}, {number(x)}, {number(y)})")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--output", type=Path, default=Path("crates/frontend/src/startup/geometry.rs"))
    args = parser.parse_args()
    source = args.source.read_text(encoding="utf-8")
    data = json.loads(re.search(r"const D = (.*?);", source).group(1))
    output = ["// Generated native vector geometry; regenerate with scripts/generate_startup_geometry.py.",
              "use super::vector::Command;", ""]
    paths = [("W", data["W"]), ("RELIEF", data["B"])] + [
        (f"LETTER_{i}", letter["d"]) for i, letter in enumerate(data["letters"])]
    for name, path in paths:
        output += [f"pub(super) const {name}: &[Command] = &[",
                   *["    " + command + "," for command in commands(path)], "];", ""]
    output += ["pub(super) const LETTERS: [&[Command]; 8] = [" +
               ", ".join(f"LETTER_{i}" for i in range(8)) + "];", ""]
    def array(name, values):
        output.append(f"pub(super) const {name}: [f64; {len(values)}] = [" +
                      ", ".join(repr(float(x)) for x in values) + "];")
    for name, key in [("START", "start"), ("END", "end")]:
        array(name, [data[key][k] for k in ["x", "y", "s"]])
    array("RING", [data["ring"][k] for k in ["cx", "cy", "r", "w"]])
    array("LINE", [data["line"][k] for k in ["x0", "x1", "y", "w"]])
    array("TEXT", [data["text"][k] for k in ["x", "y", "cap"]] + data["text"]["relief"])
    array("RELIEF_OFFSET", data["relief"])
    output.append(f"pub(super) const LETTERS_WIDTH: f64 = {float(data['lettersW'])!r};")
    output.append("pub(super) const RING_ARCS: [[f64; 2]; 2] = " + repr([[float(x) for x in arc] for arc in data["ring"]["arcs"]]) + ";")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text("\n".join(output) + "\n", encoding="utf-8")
    print(f"Generated {len(paths)} native paths")


if __name__ == "__main__":
    main()

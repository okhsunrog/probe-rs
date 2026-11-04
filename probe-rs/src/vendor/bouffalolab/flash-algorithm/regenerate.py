"""Regenerate only the algorithm block; preserve the chip definitions verbatim."""
from pathlib import Path
import subprocess
import sys

directory = Path(__file__).resolve().parent
root = directory.parents[4]
elf = directory / "target/riscv32imc-unknown-none-elf/release/bl706-flash"
tool = Path(sys.argv[1]) if len(sys.argv) > 1 else root / "target/release/target-gen"
output = subprocess.check_output([
    str(tool), "elf", str(elf), "--fixed-load-address", "--name", "bl706-mxic-1m"
], text=True)
# target-gen appends elapsed time to stdout after the YAML.
block = output.split("\nflash_algorithms:\n", 1)[1].split("\nFinished in ", 1)[0]
target = root / "probe-rs/targets/BL70x.yaml"
original = target.read_text()
target.write_text(original.split("\nflash_algorithms:\n", 1)[0]
                  + "\nflash_algorithms:\n" + block.rstrip() + "\n")

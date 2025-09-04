from collections.abc import Buffer
from dataclasses import dataclass
import hashlib
from pathlib import Path
from typing import IO
import PIL.Image


@dataclass
class Hash:
    digest: bytes

    @classmethod
    def compute(cls, buffer: Buffer) -> "Hash":
        return Hash(digest=hashlib.sha256(buffer, usedforsecurity=False).digest())

    @classmethod
    def fromhex(cls, text: str) -> "Hash":
        digest = bytes.fromhex(text)
        if len(digest) > 64:
            raise ValueError("hex digest is too long for sha256")
        return Hash(digest)

    def hexdigest(self) -> str:
        return self.digest.hex().rjust(64, "0")


@dataclass
class PngPtr:
    file_sha256: Hash
    file_size: int
    bgra_sha256: Hash
    width: int
    height: int

    @classmethod
    def from_image(cls, sha256: Hash, size: int, image: PIL.Image.Image) -> "PngPtr":
        return PngPtr(
            file_sha256=sha256,
            file_size=size,
            bgra_sha256=Hash.compute(image.convert("RGBA").tobytes("raw", "BGRa")),
            width=image.width,
            height=image.height,
        )

    def write(self, writer: IO[str]):
        writer.write(f"file {self.file_sha256.hexdigest()}\n")
        writer.write(f"filesize {self.file_size}\n")
        writer.write(f"pixels {self.bgra_sha256.hexdigest()}\n")
        writer.write(f"width {self.width}\n")
        writer.write(f"height {self.height}\n")

    @classmethod
    def read(cls, path: Path) -> "PngPtr":
        file_sha256 = None
        file_size = None
        bgra_sha256 = None
        width = None
        height = None

        with path.open("r") as f:
            while line := f.readline():
                key, value = line.split(" ", maxsplit=1)
                value = value.strip()
                match key:
                    case "file":
                        file_sha256 = Hash.fromhex(value)
                    case "filesize":
                        file_size = int(value)
                    case "pixels":
                        bgra_sha256 = Hash.fromhex(value)
                    case "width":
                        width = int(value)
                    case "height":
                        height = int(value)

        if (
            file_sha256 is None
            or file_size is None
            or bgra_sha256 is None
            or width is None
            or height is None
        ):
            raise ValueError("ptr file is incomplete")

        return PngPtr(file_sha256, file_size, bgra_sha256, width, height)


class PtrPath(Path):
    def __init__(self, path: Path):
        if not path.name.endswith(".ptr"):
            raise ValueError(
                "PtrPath can only be created from paths with names ending with .ptr"
            )

        super().__init__(path)

    @property
    def data_path(self) -> Path:
        return self.with_name(self.name.removesuffix(".ptr"))

    def read_ptr(self) -> "PngPtr":
        if not self.name.endswith(".png.ptr"):
            raise ValueError("Unsupported ptr path type")

        return PngPtr.read(self)

    def write_ptr(self, ptr: "PngPtr"):
        with self.open("w+") as f:
            ptr.write(f)

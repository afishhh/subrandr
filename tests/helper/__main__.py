#!/usr/bin/env python3
from abc import ABC, abstractmethod
import hashlib
from io import BytesIO
from pathlib import Path
import sys
from typing import Iterator, Tuple, cast
import PIL.Image
import PIL.ImageChops
import argparse
import subprocess as sp

from .ptr import Hash, PngPtr

LFS_REMOTE = "ssh://git@github.com"
LFS_REPO = "afishhh/lfs-testing"
LFS_API = f"https://lfs.github.com/{LFS_REPO}"


class Command(ABC):
    _NAME: str

    @classmethod
    def init_parser(cls, parser: argparse.ArgumentParser):
        _ = parser

    def __init__(self, namespace: argparse.Namespace):
        _ = namespace

    @abstractmethod
    def run(self):
        pass


def get_all_ptr_files() -> Iterator[Path]:
    for path in sp.check_output(["git", "ls-files", "-z"], encoding="utf8").split(
        "\x00"
    ):
        path = Path(path)
        if path.exists() and path.name.endswith(".png.ptr"):
            yield path


class PtrCreate(Command):
    _NAME = "ptr-write"

    @classmethod
    def init_parser(cls, parser: argparse.ArgumentParser):
        parser.add_argument("--stdout", action="store_true")
        parser.add_argument("path")

    def __init__(self, namespace: argparse.Namespace):
        self._stdout = namespace.stdout
        self._path = Path(namespace.path)

    def run(self):
        assert self._path.name.endswith(".png")
        content = self._path.read_bytes()
        image = PIL.Image.open(BytesIO(content))
        ptr = PngPtr.from_image(Hash.compute(content), len(content), image)
        if namespace.stdout:
            ptr.write(sys.stdout)
        else:
            with self._path.with_name(self._path.name + ".ptr").open("w+") as output:
                ptr.write(output)


class LfsPush(Command):
    _NAME = "ptr-push"

    def run(self):
        from .lfs import LfsClient, BatchObject

        ptrs: dict[str, Tuple[Path, PngPtr]] = {}
        objects = []

        for path in get_all_ptr_files():
            if not path.with_name(path.name.removesuffix(".ptr")).exists():
                print(
                    f"{path} does not have a data file, skipping",
                    file=sys.stderr,
                )
                continue

            ptr = PngPtr.read(path)
            ptrs[ptr.file_sha256.hexdigest()] = (path, ptr)
            objects.append(BatchObject(ptr.file_sha256.hexdigest(), ptr.file_size))

        if len(objects) == 0:
            print("No objects to upload", file=sys.stderr)
            return

        client = LfsClient(LFS_REMOTE, LFS_REPO, LFS_API, "upload")
        handles = client.batch(objects, "upload")

        for handle in handles:
            (ptr_path, ptr) = ptrs[handle.sha256]
            data_path = ptr_path.with_name(ptr_path.name.removesuffix(".ptr"))

            if handle.upload:
                data: bytes
                try:
                    data = data_path.read_bytes()
                except FileNotFoundError:
                    print(
                        f"{ptr_path} no longer has a data file, skipping",
                        file=sys.stderr,
                    )
                    continue

                if hashlib.sha256(data).hexdigest() != handle.sha256:
                    print(
                        f"data for {ptr_path} differs from saved hash, skipping",
                        file=sys.stderr,
                    )
                    continue

                print(
                    f"Uploading {handle.sha256} from {data_path}...",
                    end="",
                    flush=True,
                    file=sys.stderr,
                )
                client.execute_action(handle.upload, data)
                if handle.verify:
                    client.execute_action(handle.verify)
                print(" done", file=sys.stderr)


class LfsPull(Command):
    _NAME = "ptr-pull"

    @classmethod
    def init_parser(cls, parser: argparse.ArgumentParser):
        parser.add_argument("paths", nargs="*")

    def __init__(self, namespace: argparse.Namespace):
        self._paths = list(map(Path, namespace.paths))

    def get_ptr_files(self):
        if len(self._paths) == 0:
            yield from get_all_ptr_files()
        else:
            for path in self._paths:
                if path.exists() and path.name.endswith(".png.ptr"):
                    yield path
                else:
                    print(
                        f"ignoring non-existent or invalid path {path}", file=sys.stderr
                    )

    def run(self):
        from .lfs import LfsClient, BatchObject

        ptrs: dict[str, list[Path]] = {}
        objects: list[BatchObject] = []

        for path in self.get_ptr_files():
            ptr = PngPtr.read(path)
            data_path = path.with_name(path.name.removesuffix(".ptr"))

            if (
                data_path.exists()
                and Hash.compute(data_path.read_bytes()) == ptr.file_sha256
            ):
                continue

            ptrs.setdefault(ptr.file_sha256.hexdigest(), []).append(data_path)
            objects.append(BatchObject(ptr.file_sha256.hexdigest(), ptr.file_size))

        if len(objects) == 0:
            print("No objects to download", file=sys.stderr)
            return

        client = LfsClient(LFS_REMOTE, LFS_REPO, LFS_API, None)
        handles = client.batch(objects, "download")

        for handle in handles:
            paths = ptrs[handle.sha256]

            first = paths[0]
            if not handle.download:
                print(
                    f"no download action received for {handle.sha256} ({first}), skipping",
                    file=sys.stderr,
                )
                continue

            print(
                f"Downloading {handle.sha256}...", end="", flush=True, file=sys.stderr
            )
            data = client.execute_action(handle.download)
            first.write_bytes(data)
            for path in paths[1:]:
                path.hardlink_to(first)
            print(" done", file=sys.stderr)


BGRA = tuple[int, int, int, int]


def diff_images(old: PIL.Image.Image, new: PIL.Image.Image) -> PIL.Image.Image:
    max_size: tuple[int, int] = tuple(
        # pyright: ignore[reportAssignmentType]
        map(max, zip(old.size, new.size))
    )

    result = PIL.Image.new("RGBA", new.size)
    for y in range(max_size[1]):
        for x in range(max_size[0]):
            try:
                oldp = cast(BGRA, old.getpixel((x, y)))
            except IndexError:
                oldp = (0, 0, 0, 0)

            try:
                newp = cast(BGRA, new.getpixel((x, y)))
            except IndexError:
                newp = (0, 0, 0, 0)

            color_difference = (
                sum(abs(x - y) for x, y in zip(oldp[:3], newp[:3]))
            ) // 3
            alpha_difference = newp[3] - oldp[3]
            resp = [0, 0, 0, 0]
            resp[3] = max(abs(alpha_difference), color_difference)
            if alpha_difference < 0:
                resp[0] = 255
            elif alpha_difference > 0:
                resp[1] = 255
            resp[2] = color_difference
            print(resp)
            assert resp[2] == 0
            assert resp[3] == 0
            assert resp[1] == 0
            assert resp[0] == 0
            result.putpixel((x, y), tuple(resp))

    return result


class TestDiff(Command):
    _NAME = "test-diff"

    @classmethod
    def init_parser(cls, parser: argparse.ArgumentParser):
        parser.add_argument("path")

    def __init__(self, namespace: argparse.Namespace):
        self._path = Path(namespace.path)

    def run(self):
        new_path = self._path.with_suffix(".new.png")
        diff_path = self._path.with_suffix(".diff.png")
        with PIL.Image.open(self._path) as old:
            with PIL.Image.open(new_path) as new:
                difference = diff_images(old, new)
                if all(a == 0 for a in difference.getdata(3)):
                    if old.size != new.size:
                        print("Images are identical except for size")
                    else:
                        print("Images are identical")
                    return

                difference.save(diff_path)


COMMANDS: list[type[Command]] = [PtrCreate, LfsPush, LfsPull, TestDiff]

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    for command in COMMANDS:
        command.init_parser(subparsers.add_parser(command._NAME))
    namespace = parser.parse_args()
    for command in COMMANDS:
        if namespace.command == command._NAME:
            command(namespace).run()
            exit(0)
    assert False

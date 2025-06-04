# pyright: basic
from dataclasses import dataclass, field
from pathlib import Path
import json
import sys
from typing import Any


@dataclass
class Node:
    index: int = -1
    terminal: str | None = None
    cut: str = ""
    children: dict[str, "Node"] = field(default_factory=dict)


try:
    entities: dict[str, Any] = json.loads(Path("./entities.json").read_text())
except FileNotFoundError:
    print("entities.json not found", file=sys.stderr)
    print(
        "download it from https://html.spec.whatwg.org/entities.json", file=sys.stderr
    )
    exit(1)

trie = Node(0)

for entity, value in entities.items():
    characters: str = value["characters"]
    current = trie
    for chr in entity[1:]:
        if chr in current.children:
            next_node = current.children[chr]
        else:
            next_node = current.children[chr] = Node()
        current = next_node
    assert current.terminal is None
    current.terminal = characters


def cut_tree(node: Node):
    if len(node.children) == 1:
        while True:
            key, value = next(iter(node.children.items()))
            if len(value.children) == 1 and value.terminal is None:
                next_key, next_value = next(iter(value.children.items()))
                del node.children[key]
                node.children[key + next_key] = next_value
            else:
                break

    for child in node.children.values():
        cut_tree(child)


cut_tree(trie)

preorder_nodes = [trie]
next_index = 1


def index_tree(node: Node):
    global next_index
    for child in node.children.values():
        child.index = next_index
        preorder_nodes.append(child)
        next_index += 1
        index_tree(child)


index_tree(trie)


@dataclass
class Relocation:
    index: int
    address: int


DENSE_TABLE_RANGE = 74
DENSE_TABLE_BASE = ord("1")

memory = bytearray()
relocations: list[Relocation] = []
addresses: dict[int, int] = {}

ptr = 0
for node in preorder_nodes:
    addresses[node.index] = len(memory)

    terminal_len = len(node.terminal.encode()) if node.terminal is not None else 0

    next_len = len(node.children)
    if next_len >= 8:
        next_len = DENSE_TABLE_RANGE
    if next_len == 1:
        key = next(iter(node.children.keys()))
        next_len = 0x80 | len(key)

    next_off = 3 + terminal_len
    next_pad = (len(memory) + next_off) & 1
    next_off += next_pad

    memory.append(terminal_len)
    memory.append(next_len)
    memory.append(next_off)
    if node.terminal:
        memory.extend(node.terminal.encode())

    if next_len > 0:
        for _ in range(next_pad):
            memory.append(0)

    if next_len >= 0x80:
        assert next_len > 0x80
        key, target = next(iter(node.children.items()))
        relocations.append(Relocation(target.index, len(memory)))
        memory.extend([0, 0])
        memory.extend(key.encode())
    elif next_len == DENSE_TABLE_RANGE:
        offset = len(memory)
        memory.extend([0, 0] * DENSE_TABLE_RANGE)
        for key, target in node.children.items():
            ascii_off = ord(key) - DENSE_TABLE_BASE
            assert ascii_off in range(0, DENSE_TABLE_RANGE)
            ptr = offset + ascii_off * 2
            relocations.append(Relocation(target.index, ptr))
    else:
        for key, target in node.children.items():
            memory.append(ord(key))
            memory.append(0)
            relocations.append(Relocation(target.index, len(memory)))
            memory.extend([0, 0])

for relocation in relocations:
    ptr = relocation.address
    target_addr = addresses[relocation.index]
    target_addr_bytes = target_addr.to_bytes(2, "little")
    memory[ptr + 0] = target_addr_bytes[0]
    memory[ptr + 1] = target_addr_bytes[1]

Path("./trie_little_endian.bin").write_bytes(memory)

with Path("./all_entities_test.rs").open("w+") as f:
    f.write("#[allow(clippy::invisible_characters)]\n")
    f.write("\n")
    f.write("#[test]\n")
    f.write("fn all_entities() {\n")

    for entity, value in entities.items():
        entity = entity[1:]
        characters = value["characters"]
        esc = characters
        if esc == '"':
            esc = '\\"'
        elif esc == "\\":
            esc = "\\\\"
        f.write(
            f'\tassert_eq!(super::consume(b"{entity}"), Some(("{esc}", {len(entity)})));\n'
        )

    f.write("}\n")

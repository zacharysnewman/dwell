#!/usr/bin/env python3
"""Reference encoder for protocol golden vectors (independent of the C++ and TS codecs).

Writes shared/protocol/vectors.txt. Each line is `<name> <hex>`; names starting with `!` are
malformed inputs that every decoder must reject. The field values used here are mirrored in the
C++ (server/tests/protocol_test.cpp) and TS (client/src/protocol/messages.test.ts) tests.
"""
import json
import struct
from pathlib import Path

here = Path(__file__).parent
c = json.loads((here / "constants.json").read_text())
T = c["messageTypes"]


def s(text: str) -> bytes:
    b = text.encode("utf-8")
    return struct.pack("<H", len(b)) + b


B = c["inputButtons"]
CF = c["controllerFlags"]
PF = c["playerFlags"]
PS = c["playerStates"]
GK = c["groundKinds"]
EK = c["playerEventKinds"]
DC = c["damageCauses"]


def f32s(*values: float) -> bytes:
    return struct.pack("<" + "f" * len(values), *values)


def f64s(*values: float) -> bytes:
    return struct.pack("<" + "d" * len(values), *values)


def posfix(*values: float) -> bytes:
    # i32 in 1/positionFixedScale m, nearest (halves up); these test values are exact.
    scale = c["world"]["positionFixedScale"]
    return struct.pack("<" + "i" * len(values), *(int(v * scale) for v in values))


def f16s(*values: float) -> bytes:
    return struct.pack("<" + "e" * len(values), *values)


def input_frame(seq, mx, my, buttons, yaw, pitch):
    return struct.pack("<IbbHhh", seq, mx, my, buttons, yaw, pitch)


def controller(flags, ladder=None, released=None):
    out = struct.pack("<B", flags)
    out += f32s(1.5, -2.25, 0.5, 0.0, 2.0, -2.25, 3.0, 0.0, -1.0, 0.25)
    out += struct.pack("<BHBBB", GK["Player"], 7, 11, 5, 2)
    if ladder is not None:
        out += struct.pack("<iii", *ladder)
    if released is not None:
        out += struct.pack("<ii", *released)
    return out


def snapshot_local(flags, health, state, ctrl, input_buffer=0, knockback=0):
    # pos64: a position near the rim of the 8,192 km world keeps its millimetres.
    return (f64s(8191999.125, 0.9, -3.25) + f32s(5.0, -0.5, 0.0)
            + struct.pack("<BBBBI", flags, health, state, input_buffer, knockback) + ctrl)


PLAYER_INPUT = (
    struct.pack("<BIB", T["PlayerInput"], 300, 2)
    + input_frame(41, 127, -127, B["jump"] | B["run"], -16384, 32767)
    + input_frame(42, 0, 90, B["crouch"] | B["fly"], 12345, -100)
)
SNAPSHOT = (
    struct.pack("<BII", T["PhysicsSnapshot"], 603, 42)
    + snapshot_local(
        PF["grounded"] | PF["climbing"], 87, PS["Climbing"],
        controller(CF["grounded"] | CF["climbing"] | CF["hasReleased"], (-5, 64, 1000000), (-5, 7)),
        input_buffer=3, knockback=40,
    )
    + struct.pack("<B", 2)
    + struct.pack("<H", 3) + posfix(1.0, 2.0, 3.0) + f16s(0.5, -8.0, 0.000061035156)
    + struct.pack("<hhBB", 16384, -8192, PS["Running"], PF["grounded"])
    + struct.pack("<H", 9) + posfix(-1.0, 0.00390625, 8192000.5) + f16s(65504.0, -0.0, 1.0)
    + struct.pack("<hhBB", 0, 0, PS["Swimming"], PF["swimming"] | PF["dead"])
)
SNAPSHOT_MIN = (
    struct.pack("<BII", T["PhysicsSnapshot"], 3, 0)
    + snapshot_local(0, 100, PS["Idle"], controller(0))
    + struct.pack("<B", 0)
)
EVENT_HEAD = lambda kind: struct.pack("<BBHII", T["PlayerEvent"], kind, 5, 1200, 77)

CF_ = c["chunkForms"]
N = c["chunkSize"]


def chunk_layered(x, y, z):
    """Test chunk "layered": stone below y 10, grass at 10, one leaves voxel, one id 300."""
    if (x, y, z) == (3, 20, 7):
        return 17
    if (x, y, z) == (31, 31, 31):
        return 300
    return 2 if y < 10 else 4 if y == 10 else 0


def chunk_wide(x, y, z):
    """Test chunk "wide": 300 distinct materials on the bottom layer (u16 palette indices)."""
    return (x + N * z) % 300 + 1 if y == 0 else 0


def varint(v):
    out = b""
    while v >= 0x80:
        out += bytes([(v & 0x7F) | 0x80])
        v >>= 7
    return out + bytes([v])


def chunk_voxels(material):
    """Palette + RLE in layer order (x fastest, then z, then y)."""
    wire = [material(x, y, z) for y in range(N) for z in range(N) for x in range(N)]
    palette = list(dict.fromkeys(wire))
    wide = len(palette) > 256
    out = struct.pack("<H", len(palette)) + b"".join(struct.pack("<H", m) for m in palette)
    i = 0
    while i < len(wire):
        j = i
        while j < len(wire) and wire[j] == wire[i]:
            j += 1
        idx = palette.index(wire[i])
        out += varint(j - i) + (struct.pack("<H", idx) if wide else bytes([idx]))
        i = j
    return out


def chunk_head(form, coord, revision):
    return struct.pack("<BBiiiI", T["ChunkData"], form, *coord, revision)


EXPLICIT = chunk_head(CF_["Explicit"], (-1, 2, 70000), 5) + chunk_voxels(chunk_layered)
PAL1 = chunk_head(CF_["Explicit"], (0, 0, 0), 1) + struct.pack("<HH", 1, 2)

BA = c["blockEditActions"]
VR = c["voxelModificationReasons"]


def block_edit(action, cell, face, material=None):
    out = struct.pack("<BBiiiB", T["BlockEditRequest"], action, *cell, face)
    return out + (struct.pack("<H", material) if material is not None else b"")


def voxel_modification(reason, tick, chunks):
    """chunks: [(coord, newRevision, [(localIndex, material), ...]), ...]"""
    out = struct.pack("<BBIH", T["VoxelModification"], reason, tick, len(chunks))
    for coord, revision, changes in chunks:
        out += struct.pack("<iiiIH", *coord, revision, len(changes))
        out += b"".join(struct.pack("<HH", i, m) for i, m in changes)
    return out


VOXEL_MOD = voxel_modification(VR["Edit"], 4242, [
    ((-1, 2, 256000), 7, [(0, 0), (32767, 300)]),
    ((5, -64, -3), 1, [(1 | (2 << 5) | (3 << 10), 16)]),
])

LF = c["lodForms"]
LOD_PAD = c["lod"]["sectionCells"] + 2


def lod_cell(x, y, z):
    """Test LOD section "strata" (cells −1..32 with the apron): bedrock under the section, stone,
    a grass layer, water in one corner above it, an apron column of leaves at x = −1."""
    if y == -1:
        return 1
    if x == -1 and z == 5:
        return 17
    return 2 if y < 12 else 4 if y == 12 else 10 if y < 16 and x > 20 else 0


def lod_cells(cell):
    """Palette + RLE over the 34³ cells, already in layer order (x fastest, then z, then y)."""
    r = range(-1, LOD_PAD - 1)
    wire = [cell(x, y, z) for y in r for z in r for x in r]
    palette = list(dict.fromkeys(wire))
    out = struct.pack("<H", len(palette)) + b"".join(struct.pack("<H", m) for m in palette)
    i = 0
    while i < len(wire):
        j = i
        while j < len(wire) and wire[j] == wire[i]:
            j += 1
        out += varint(j - i) + bytes([palette.index(wire[i])])
        i = j
    return out


def lod_index(flags, entries):
    out = struct.pack("<BBI", T["LodIndex"], flags, len(entries))
    return out + b"".join(struct.pack("<iiI", *e) for e in entries)


def lod_update(entries):
    out = struct.pack("<BH", T["LodIndexUpdate"], len(entries))
    return out + b"".join(struct.pack("<iiI", *e) for e in entries)


def lod_request(sections):
    out = struct.pack("<BB", T["LodRequest"], len(sections))
    return out + b"".join(struct.pack("<BiiiI", level, *s, known) for level, s, known in sections)


def lod_head(form, level, section, revision):
    return struct.pack("<BBBiiiI", T["LodData"], form, level, *section, revision)


LOD_EXPLICIT = lod_head(LF["Explicit"], 1, (131072, 32, 131071), 42) + lod_cells(lod_cell)

PK = bytes(range(32))
NONCE = bytes(range(0xA0, 0xC0))
SIG = bytes((i * 3) & 0xFF for i in range(64))
BINDING = bytes(range(0x10, 0x30))

vectors = {
    "datagram_ping": struct.pack("<BId", T["DatagramPing"], 0x01020304, 1234.5),
    "datagram_pong": struct.pack("<BIdI", T["DatagramPong"], 7, 0.25, 600),
    "status_request": struct.pack("<B", T["StatusRequest"]),
    "status_response": struct.pack("<BH", T["StatusResponse"], 1)
    + s("Dwell Test")
    + s("héllo ✓")
    + struct.pack("<HHB", 3, 8, 1),
    "client_hello": struct.pack("<BH", T["ClientHello"], 1) + s("0.1.0") + PK + s("Zack"),
    "challenge": struct.pack("<B", T["Challenge"]) + NONCE,
    "client_auth": struct.pack("<B", T["ClientAuth"]) + SIG,
    "welcome": struct.pack("<BHQIIiiiB", T["Welcome"], 42, 0x0123456789ABCDEF, 7, 123456,
                           -3, 2, 1000000, c["welcomeFlags"]["flight"]),
    "reject": struct.pack("<BB", T["Reject"], c["rejectReasons"]["ProtocolVersion"])
    + s("Server runs protocol 2"),
    "ping": struct.pack("<BId", T["Ping"], 9, 1000.0),
    "pong": struct.pack("<BIdId", T["Pong"], 9, 1000.0, 60, 5000.125),
    "auth_transcript": c["authDomainTag"].encode() + NONCE + BINDING + PK,
    "player_input": PLAYER_INPUT,
    "physics_snapshot": SNAPSHOT,
    "physics_snapshot_min": SNAPSHOT_MIN,
    "player_event_knockback": EVENT_HEAD(EK["Knockback"]) + f32s(0.0, 14.0, -0.5),
    "player_event_damage": EVENT_HEAD(EK["Damage"]) + struct.pack("<BB", 17, DC["Fall"]),
    "player_event_death": EVENT_HEAD(EK["Death"]) + struct.pack("<B", DC["Crush"]),
    "player_event_respawn": EVENT_HEAD(EK["Respawn"]) + f64s(7999488.5, 12.0, -0.25),
    "worldgen_check": struct.pack("<BQ", T["WorldgenCheck"], 0xFEDCBA9876543210),
    "chunk_data_generated": chunk_head(CF_["Generated"], (4, -2, -9), 0),
    "chunk_data_air": chunk_head(CF_["Air"], (256000, 191, -3), 0),
    "chunk_data_explicit": EXPLICIT,
    "chunk_data_explicit_wide": chunk_head(CF_["Explicit"], (1, 1, 1), 3)
    + chunk_voxels(chunk_wide),
    "chunk_data_explicit_solid": PAL1 + varint(N ** 3) + b"\x00",
    "chunk_unload": struct.pack("<BH", T["ChunkUnload"], 2)
    + struct.pack("<iiiiii", 1, 2, 3, -4, -5, 2000000),
    "block_edit_break": block_edit(BA["Break"], (8191999, -2048, -12), 3),
    "block_edit_place": block_edit(BA["Place"], (-7, 64, 3), 4, 16),
    "voxel_modification": VOXEL_MOD,
    "chunk_resync": struct.pack("<BH", T["ChunkResync"], 2)
    + struct.pack("<iiiiii", 0, -1, 2, 256000, 191, -256000),
    "lod_index": lod_index(1, [(1024, 1023, 7), (0, 2047, 4000000000)]),
    "lod_index_empty": lod_index(1, []),
    "lod_index_part": lod_index(0, [(-5, 3, 1)]),
    "lod_index_update": lod_update([(5, 6, 9)]),
    "lod_request": lod_request([(3, (-4, 17, 8), 0), (19, (0, 0, 0), 12)]),
    "lod_data_generated": lod_head(LF["Generated"], 5, (100, 3, -2), 0),
    "lod_data_unchanged": lod_head(LF["Unchanged"], 12, (1, 0, 2), 77),
    "lod_data_explicit": LOD_EXPLICIT,
}

malformed = {
    "!empty": b"",
    "!unknown_type": bytes([0xEE]),
    "!truncated_ping": vectors["ping"][:-1],
    "!trailing_byte": vectors["welcome"] + b"\x00",
    "!bad_utf8": struct.pack("<BH", T["ClientHello"], 1)
    + struct.pack("<H", 2) + b"\xC3\x28" + PK + s("Zack"),
    "!name_too_long": struct.pack("<BH", T["ClientHello"], 1)
    + s("0.1.0") + PK + s("x" * (c["limits"]["displayNameMaxBytes"] + 1)),
    "!bad_reject_reason": struct.pack("<BB", T["Reject"], 0) + s("x"),
    "!input_count_zero": struct.pack("<BIB", T["PlayerInput"], 0, 0),
    "!input_count_five": struct.pack("<BIB", T["PlayerInput"], 0, 5)
    + input_frame(1, 0, 0, 0, 0, 0) * 5,
    "!input_unknown_button": struct.pack("<BIB", T["PlayerInput"], 0, 1)
    + input_frame(1, 0, 0, 0x80, 0, 0),
    "!snapshot_bad_state": SNAPSHOT_MIN[:9 + 38] + b"\x63" + SNAPSHOT_MIN[9 + 39:],
    "!snapshot_truncated_remote": SNAPSHOT[:-1],
    "!snapshot_missing_ladder": SNAPSHOT_MIN[:9 + 44] + bytes([CF["climbing"]]) + SNAPSHOT_MIN[9 + 45:],
    "!snapshot_position_nan": SNAPSHOT_MIN[:9] + f64s(float("nan")) + SNAPSHOT_MIN[9 + 8:],
    "!respawn_out_of_range": EVENT_HEAD(EK["Respawn"]) + f64s(4e7, 0.0, 0.0),
    "!event_bad_kind": EVENT_HEAD(0) + f32s(0, 0, 0),
    "!event_bad_cause": EVENT_HEAD(EK["Damage"]) + struct.pack("<BB", 1, 0),
    "!chunk_bad_form": chunk_head(3, (0, 0, 0), 0),
    "!chunk_air_payload": chunk_head(CF_["Air"], (0, 0, 0), 0) + b"\x00",
    "!chunk_generated_payload": chunk_head(CF_["Generated"], (0, 0, 0), 0) + b"\x00",
    "!chunk_empty_palette": chunk_head(CF_["Explicit"], (0, 0, 0), 0) + struct.pack("<H", 0),
    "!chunk_short": PAL1 + varint(N ** 3 - 1) + b"\x00",
    "!chunk_overrun": PAL1 + varint(N ** 3 - 1) + b"\x00" + varint(2) + b"\x00",
    "!chunk_zero_run": PAL1 + varint(0) + b"\x00" + varint(N ** 3) + b"\x00",
    "!chunk_bad_index": PAL1 + varint(N ** 3) + b"\x01",
    "!chunk_overlong_varint": PAL1 + b"\x81\x00" + b"\x00" + varint(N ** 3 - 1) + b"\x00",
    "!chunk_truncated": EXPLICIT[:-1],
    "!unload_empty": struct.pack("<BH", T["ChunkUnload"], 0),
    "!block_edit_bad_action": block_edit(3, (0, 0, 0), 0),
    "!block_edit_bad_face": block_edit(BA["Break"], (0, 0, 0), 6),
    "!block_edit_break_material": block_edit(BA["Break"], (0, 0, 0), 0, 2),
    "!block_edit_place_no_material": block_edit(BA["Place"], (0, 0, 0), 0),
    "!voxel_mod_bad_reason": voxel_modification(0, 1, [((0, 0, 0), 1, [(0, 1)])]),
    "!voxel_mod_no_chunks": voxel_modification(VR["Edit"], 1, []),
    "!voxel_mod_no_changes": voxel_modification(VR["Edit"], 1, [((0, 0, 0), 1, [])]),
    "!voxel_mod_bad_index": voxel_modification(VR["Edit"], 1, [((0, 0, 0), 1, [(32768, 1)])]),
    "!voxel_mod_truncated": VOXEL_MOD[:-1],
    "!lod_index_bad_flags": lod_index(2, []),
    "!lod_index_truncated": lod_index(1, [(1, 2, 3)])[:-1],
    "!lod_update_empty": lod_update([]),
    "!lod_request_empty": lod_request([]),
    "!lod_request_too_many": lod_request([(1, (0, 0, 0), 0)] * (c["lod"]["maxRequestSections"] + 1)),
    "!lod_request_level_zero": lod_request([(0, (0, 0, 0), 0)]),
    "!lod_request_level_too_high": lod_request([(c["lod"]["maxLevel"] + 1, (0, 0, 0), 0)]),
    "!lod_data_bad_form": lod_head(3, 1, (0, 0, 0), 0),
    "!lod_data_bad_level": lod_head(LF["Generated"], c["lod"]["maxLevel"] + 1, (0, 0, 0), 0),
    "!lod_data_unchanged_payload": lod_head(LF["Unchanged"], 1, (0, 0, 0), 1) + b"\x00",
    "!lod_data_truncated": LOD_EXPLICIT[:-1],
    "!resync_empty": struct.pack("<BH", T["ChunkResync"], 0),
    "!resync_too_many": struct.pack("<BH", T["ChunkResync"], c["limits"]["maxResyncChunks"] + 1)
    + struct.pack("<iii", 0, 0, 0) * (c["limits"]["maxResyncChunks"] + 1),
}

lines = ["# Generated by shared/protocol/make_vectors.py. Do not edit."]
lines += [f"{k} {v.hex()}" for k, v in {**vectors, **malformed}.items()]
(here / "vectors.txt").write_text("\n".join(lines) + "\n")

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
    return (f32s(10.5, 0.9, -3.25) + f32s(5.0, -0.5, 0.0)
            + struct.pack("<BBBBI", flags, health, state, input_buffer, knockback) + ctrl)


PLAYER_INPUT = (
    struct.pack("<BIB", T["PlayerInput"], 300, 2)
    + input_frame(41, 127, -127, B["jump"] | B["run"], -16384, 32767)
    + input_frame(42, 0, 90, B["crouch"], 12345, -100)
)
SNAPSHOT = (
    struct.pack("<BII", T["PhysicsSnapshot"], 603, 42)
    + snapshot_local(
        PF["grounded"] | PF["climbing"], 87, PS["Climbing"],
        controller(CF["grounded"] | CF["climbing"] | CF["hasReleased"], (-5, 64, 1000000), (-5, 7)),
        input_buffer=3, knockback=40,
    )
    + struct.pack("<B", 2)
    + struct.pack("<H", 3) + f32s(1.0, 2.0, 3.0) + f16s(0.5, -8.0, 0.000061035156)
    + struct.pack("<hhBB", 16384, -8192, PS["Running"], PF["grounded"])
    + struct.pack("<H", 9) + f32s(-1.0, 0.0, 65504.0) + f16s(65504.0, -0.0, 1.0)
    + struct.pack("<hhBB", 0, 0, PS["Swimming"], PF["swimming"] | PF["dead"])
)
SNAPSHOT_MIN = (
    struct.pack("<BII", T["PhysicsSnapshot"], 3, 0)
    + snapshot_local(0, 100, PS["Idle"], controller(0))
    + struct.pack("<B", 0)
)
EVENT_HEAD = lambda kind: struct.pack("<BBHII", T["PlayerEvent"], kind, 5, 1200, 77)

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
    "welcome": struct.pack("<BHQII", T["Welcome"], 42, 0x0123456789ABCDEF, 7, 123456),
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
    "player_event_respawn": EVENT_HEAD(EK["Respawn"]) + f32s(0.5, 0.0, 0.5),
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
    "!snapshot_bad_state": SNAPSHOT_MIN[:9 + 26] + b"\x63" + SNAPSHOT_MIN[9 + 27:],
    "!snapshot_truncated_remote": SNAPSHOT[:-1],
    "!snapshot_missing_ladder": SNAPSHOT_MIN[:9 + 32] + bytes([CF["climbing"]]) + SNAPSHOT_MIN[9 + 33:],
    "!event_bad_kind": EVENT_HEAD(0) + f32s(0, 0, 0),
    "!event_bad_cause": EVENT_HEAD(EK["Damage"]) + struct.pack("<BB", 1, 0),
}

lines = ["# Generated by shared/protocol/make_vectors.py. Do not edit."]
lines += [f"{k} {v.hex()}" for k, v in {**vectors, **malformed}.items()]
(here / "vectors.txt").write_text("\n".join(lines) + "\n")

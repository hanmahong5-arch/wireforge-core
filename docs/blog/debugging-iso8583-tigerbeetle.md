# Debugging ISO 8583 in a TigerBeetle Pipeline: A Field Guide

*Draft, 2026-05-20. Target outlets: TigerBeetle Discord `#community`,
TigerBeetle blog (if accepted), Wireforge docs site.*

---

If you run a card-payment processor on top of TigerBeetle, you've
already accepted the bet that **financial accounting belongs in a
dedicated OLTP database**, not in a general-purpose RDBMS. Account
debits and credits live as TigerBeetle transfers; consistency is
enforced by the database, not the application.

That leaves a different unsolved problem: **how does an ISO 8583
message become a TigerBeetle transfer in the first place, and how do
you debug it when the field decoding is wrong?**

This post walks through that pipeline with a real-shaped (synthetic)
ISO 8583 hex message, decodes it with the `wf` CLI, and shows where
to plug a TigerBeetle `create_transfers` call so the audit trail is
byte-exact.

## The shape of the problem

ISO 8583 is a bitmapped wire protocol from 1987 (revised 1993 and
2003). A typical authorization request looks like this on the wire:

```
303230302000000000000000303030303030
└──┬──┘└──────┬──────┘└──────┬──────┘
   │          │              │
   MTI        Bitmap         Field 3 (Processing Code)
   "0200"     Field 3 set    "000000"
   (auth req) only
```

Real messages set 8–20 fields and run 200+ bytes. The bitmap tells
you which fields are present; the field table (105 well-defined slots
in the 1987 spec) tells you what each field means.

The mapping you need for a TigerBeetle pipeline:

| ISO 8583 field        | TigerBeetle column / use                |
|-----------------------|-----------------------------------------|
| MTI                   | `Transfer.code` (or out-of-band)        |
| Field 2 (PAN)         | `Account.user_data_128` (hashed)        |
| Field 3 (Proc Code)   | Routing — credit vs debit vs reversal   |
| Field 4 (Amount)      | `Transfer.amount`                       |
| Field 7 (Trans D&T)   | `Transfer.timestamp` (with skew check)  |
| Field 11 (STAN)       | `Transfer.id` (deterministic from STAN) |
| Field 32 (Acquirer)   | Account routing — debit account ledger  |
| Field 41 (Terminal)   | `Transfer.user_data_64`                 |
| Field 52 (PIN block)  | Out-of-band — never on the ledger       |

The instinct is to write a parser, push it through a sea of `if`
statements, and call it done. The instinct is wrong, for two reasons:

1. **ISO 8583 fields are stringly-typed on the wire**. A "numeric"
   field can legally contain non-digit bytes if a partner gateway
   misbehaves. A length-prefixed field can lie about its own length.
   Your parser needs to refuse silently-broken messages, not
   propagate them into TigerBeetle transfers that won't reverse.
2. **Round-trip determinism is the only safe debug signal**. If you
   can't `parse(bytes) → build(msg) == bytes` for a captured
   production message, then your interpretation of the message is
   wrong somewhere, and any TigerBeetle write you derive from it is
   suspect.

## Debugging loop with `wf`

[`wf`](https://github.com/wireforge/wireforge-core) is a small
Apache-2.0 Rust CLI that gives you a deterministic parse/build of
ISO 8583 ASCII messages. Install:

```bash
cargo install wf-cli
```

### Step 1: tree-view a captured message

Suppose your TigerBeetle pipeline rejected a message and the
operator pasted a hex blob into Slack. First check it parses:

```bash
$ wf parse 303230302000000000000000303030303030
ISO 8583 Message
├── MTI: 0200
├── Bitmap: 2000000000000000
│   └── Fields set: 3
└── Fields:
    └── [  3] Processing Code — n6 fixed (6 bytes)
            "000000"
```

Field 3 = `000000` is "Goods and Services / Debit from cardholder
account / Credit to acquirer account". Now you know: this should
become **two** TigerBeetle transfers (debit cardholder, credit
acquirer), not one.

### Step 2: round-trip check

Before writing anything to TigerBeetle, confirm the message is
canonical:

```bash
$ wf parse --json 303230302000000000000000303030303030 \
    | jq . \
    | wf build
303230302000000000000000303030303030
```

Equal input and output. If they differ, your captured hex is from a
non-canonical encoder (or your transport ate a byte) — investigate
before posting to the ledger.

### Step 3: construct a TigerBeetle transfer

With a parsed message in JSON form (`wf parse --json ...`), the
mapping into TigerBeetle becomes a 30-line function. Pseudocode:

```python
import struct, hashlib
from tb_client import Transfer

msg = parse_iso8583_json(captured_hex)
stan = msg["fields"][11]["value_ascii"]
amount = int(msg["fields"][4]["value_ascii"])
proc = msg["fields"][3]["value_ascii"]
ts = parse_iso8583_datetime(msg["fields"][7]["value_ascii"])

# Deterministic ID from STAN so retries dedupe naturally.
transfer_id = int.from_bytes(
    hashlib.sha256(f"acq:{stan}:{ts}".encode()).digest()[:16],
    "big",
)

debit_account_id  = cardholder_account_for(msg["fields"][2])
credit_account_id = acquirer_settlement_account_for(msg["fields"][32])

tb_client.create_transfers([
    Transfer(
        id=transfer_id,
        debit_account_id=debit_account_id,
        credit_account_id=credit_account_id,
        amount=amount,
        ledger=USD_LEDGER,
        code=int(msg["mti"]),  # e.g. 200 for auth req
        timestamp=ts,
    ),
])
```

TigerBeetle's `create_transfers` is idempotent on `id`, so a retry
of the same STAN produces the same transfer ID and is silently
deduped. That property is **why** the deterministic hash matters —
you cannot let two equivalent ISO 8583 messages produce two
TigerBeetle rows.

## What this doesn't solve

`wf` is structure-only:

- It does NOT verify a PAN's Luhn checksum.
- It does NOT enforce that a "numeric" field contains only digits.
- It does NOT check that field 49 is a real ISO 4217 currency.
- It does NOT verify the MAC (field 64 / 128) — that needs your HSM.

Those are application-layer checks. The point of `wf` is to give
you a deterministic, debuggable boundary between "bytes on the
wire" and "intent the application can reason about", so that when
the TigerBeetle write fails or reverses, you can point to the
exact byte that caused it.

## What I'd love help on

I'm looking for:

- **Sanitized real-shape ISO 8583 hex samples** — PAN replaced with
  `400000xxxxxxxx0002` style, MAC zeroed. Even five samples from
  different acquirers would catch dialect bugs in the 1987 spec
  implementation.
- **Schema-override examples** for fields 105..=127 (reserved /
  national / private). If you have a private-use field convention
  you can share, I'd like to add a documented override path to the
  field table.
- **Feedback on the pipeline shape above**. Is the STAN-hash ID
  scheme robust under your replay scenarios? Are there ISO 8583
  conventions I'm missing for the dedupe key?

Reach me on the TigerBeetle Discord `#community` channel as
`@wireforge`, or open an issue at
[wireforge/wireforge-core](https://github.com/wireforge/wireforge-core).

---

*Wireforge is Apache-2.0 OSS. The `wf` CLI parses and builds
ISO 8583 ASCII messages with strict round-trip guarantees, and a
companion MCP server exposes the same codec to Claude / Cursor /
hermes-agent for AI-assisted debugging.*

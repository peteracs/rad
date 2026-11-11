#!/usr/bin/env python3
"""Write the stockroom incident session: one working day at the order desk.

Every command is one a clerk could legitimately type. Exactly one sequence
hits the bug in stockroom.rad: order o17 ships, and a while later its
customer cancels it. The day goes on around it, with later activity on the
same sku, so the audit at `done` is far from the cause.

    python projects/dogfood/stockroom/make_session.py

writes session.txt next to this script.
"""
import pathlib
import random

SKUS = ["bolt", "nut", "washer", "gear", "belt", "chain", "hinge", "spring"]
LINES = 400
INCIDENT = "o17"
SHIP_AFTER = 14  # lines between placing the incident order and shipping it
CANCEL_AFTER = 9  # lines between shipping it and the customer's call


def main() -> None:
    rng = random.Random(20261111)
    out = []
    on_hand = {sku: 0 for sku in SKUS}
    open_orders = {}  # id -> (sku, qty)
    next_id = 1
    due = {}  # line count -> command for the incident order

    for sku in SKUS:
        qty = rng.randrange(200, 400)
        out.append(f"receive {sku} {qty}")
        on_hand[sku] += qty

    while len(out) < LINES - 1:
        if len(out) in due:
            out.append(due.pop(len(out)))
            continue
        # The incident order is the clerk's, not the dice's, to ship or cancel.
        routine = sorted(order for order in open_orders if order != INCIDENT)
        roll = rng.random()
        if roll < 0.18:
            sku = rng.choice(SKUS)
            qty = rng.randrange(20, 120)
            out.append(f"receive {sku} {qty}")
            on_hand[sku] += qty
        elif roll < 0.58:
            sku = rng.choice(SKUS)
            qty = rng.randrange(5, 60)
            order = f"o{next_id}"
            next_id += 1
            if order == INCIDENT:
                qty = min(qty, on_hand[sku])
                due[len(out) + 1 + SHIP_AFTER] = f"ship {order}"
                due[len(out) + 1 + SHIP_AFTER + CANCEL_AFTER] = f"cancel {order}"
            out.append(f"order {order} {sku} {qty}")
            if on_hand[sku] >= qty:
                on_hand[sku] -= qty
                open_orders[order] = (sku, qty)
        elif roll < 0.86 and routine:
            order = rng.choice(routine)
            out.append(f"ship {order}")
            del open_orders[order]
        elif routine:
            order = rng.choice(routine)
            out.append(f"cancel {order}")
            sku, qty = open_orders.pop(order)
            on_hand[sku] += qty

    assert not due, "the day ended before the incident played out"
    out.append("done")
    # newline="\n": the session is program input, LF on every platform.
    target = pathlib.Path(__file__).with_name("session.txt")
    with open(target, "w", encoding="ascii", newline="\n") as handle:
        handle.write("\n".join(out) + "\n")


if __name__ == "__main__":
    main()

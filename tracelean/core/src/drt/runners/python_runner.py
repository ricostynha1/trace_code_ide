#!/usr/bin/env python3
"""TraceLean's differential-testing runner for Python implementations.

This file is *mechanism*, not project code. It is shipped inside TraceLean and
written into `.tracelean/drt/` when a run starts, so a project that binds a Lean
model to Python functions needs no hand-written harness at all: the binding in
`.tracelean/drt.json` says which function to call and how the model's field
names map onto its parameters, and this runner does the calling.

The design follows Cedar's: one harness for the whole system, driven by a
declarative description of each entry point, rather than one adapter per
property. A hand-written adapter remains available for the case the binding
cannot describe -- a datatype whose JSON shape genuinely does not match -- but
it is the exception, not the price of admission.

Protocol: one JSON case per line on stdin, one JSON reply per line on stdout.

    <- {"case": 7, "op": "REQ-CHECKOUT.total", "input": {...}}
    -> {"case": 7, "output": {...}}          on success
    -> {"case": 7, "error": "ValueError: …"} on a raise

A raise is an answer, not an abort: the model may fail on that input too, and
"both fail" is agreement while "one fails" is a divergence worth reporting.
"""
from __future__ import annotations

import argparse
import dataclasses
import importlib.util
import inspect
import json
import os
import sys


def load_callable(root, entry):
    """Resolve `path/to/module.py::symbol` to a callable.

    Loading by *path* rather than by module name is deliberate: TraceLean has no
    filename or package conventions, so the implementation may live anywhere in
    the project, and importing it must not depend on the project being an
    installed package.
    """
    if "::" not in entry:
        raise ValueError(f"entry {entry!r} must be of the form path/to/file.py::symbol")
    rel_path, symbol = entry.split("::", 1)
    path = os.path.join(root, rel_path)
    if not os.path.isfile(path):
        raise FileNotFoundError(f"{path}: no such file (entry {entry!r})")

    # The implementation's own directory goes on sys.path so its sibling
    # imports resolve the way they do when the project runs normally.
    directory = os.path.dirname(os.path.abspath(path))
    if directory not in sys.path:
        sys.path.insert(0, directory)

    name = "tracelean_drt_" + os.path.splitext(os.path.basename(path))[0]
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)

    target = module
    for part in symbol.split("."):
        target = getattr(target, part)
    if not callable(target):
        raise TypeError(f"{entry}: {symbol} is not callable")
    return target


def jsonable(value):
    """Coerce a result into something comparable with the model's JSON.

    The model's output is whatever Lean's `deriving ToJson` produced -- objects
    with named fields, numbers, booleans, lists. Most Python return values are
    already that. Dataclasses, named tuples and plain objects are not, and
    converting them here is what keeps the common case free of a hand-written
    adapter. Anything this cannot convert is left alone, so the failure is a
    clear serialization error naming the type rather than a silent mismatch.
    """
    if dataclasses.is_dataclass(value) and not isinstance(value, type):
        return {k: jsonable(v) for k, v in dataclasses.asdict(value).items()}
    if hasattr(value, "_asdict"):          # collections.namedtuple
        return {k: jsonable(v) for k, v in value._asdict().items()}
    if isinstance(value, dict):
        return {k: jsonable(v) for k, v in value.items()}
    if isinstance(value, (list, tuple)):
        return [jsonable(v) for v in value]
    if isinstance(value, (str, int, float, bool)) or value is None:
        return value
    if hasattr(value, "__dict__"):
        return {k: jsonable(v) for k, v in vars(value).items() if not k.startswith("_")}
    return value


class Entry:
    """One op: which function to call, and how to spell its arguments."""

    def __init__(self, root, config):
        self.op = config["op"]
        self.function = load_callable(root, config["entry"])
        # Model field name -> implementation parameter name. Absent keys pass
        # through unchanged, which is the case when both sides already agree.
        self.params = config.get("params") or {}
        self.convert = (
            load_callable(root, config["convert"]) if config.get("convert") else None
        )
        self.signature = inspect.signature(self.function)

    def call(self, payload):
        if not isinstance(payload, dict):
            # A model taking a single scalar sends the bare value.
            result = self.function(payload)
            return jsonable(self.convert(result) if self.convert else result)

        kwargs = {self.params.get(k, k): v for k, v in payload.items()}
        try:
            self.signature.bind(**kwargs)
            result = self.function(**kwargs)
        except TypeError:
            # Positional fallback: the implementation's parameters may simply be
            # named differently with no mapping given. Binding by declaration
            # order is a guess, so it is reported by `tracelean drt bind` rather
            # than made silently -- but it is the guess that usually works.
            names = [
                p.name
                for p in self.signature.parameters.values()
                if p.kind
                in (p.POSITIONAL_ONLY, p.POSITIONAL_OR_KEYWORD)
            ]
            values = list(payload.values())
            if len(names) != len(values):
                raise
            result = self.function(*values)
        return jsonable(self.convert(result) if self.convert else result)


def build_table(root, bindings):
    """Load every op this runner can serve.

    A binding that fails to load is recorded rather than fatal: one broken
    binding must not take down a run of the five that work, and the case that
    asks for it gets an `error` reply naming the reason.
    """
    table, broken = {}, {}
    for binding in bindings:
        try:
            table[binding["op"]] = Entry(root, binding)
        except Exception as exc:
            broken[binding["op"]] = f"{type(exc).__name__}: {exc}"
    return table, broken


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, help="project root")
    parser.add_argument("--bindings", required=True, help="JSON file describing the ops")
    args = parser.parse_args()

    with open(args.bindings, encoding="utf-8") as handle:
        bindings = json.load(handle)
    table, broken = build_table(args.root, bindings)

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            case = json.loads(line)
        except json.JSONDecodeError as exc:
            sys.stdout.write(json.dumps({"case": 0, "error": f"bad case: {exc}"}) + "\n")
            sys.stdout.flush()
            continue

        op = case.get("op")
        identifier = case.get("case", 0)
        if op in broken:
            reply = {"case": identifier, "error": f"binding for {op}: {broken[op]}"}
        elif op not in table:
            reply = {"case": identifier, "error": f"unknown op: {op}"}
        else:
            try:
                reply = {"case": identifier, "output": table[op].call(case.get("input"))}
            except Exception as exc:
                reply = {"case": identifier, "error": f"{type(exc).__name__}: {exc}"}

        sys.stdout.write(json.dumps(reply) + "\n")
        # The pipe is block-buffered. Without this flush the comparator waits
        # for a reply that is sitting in this process's buffer, and the run
        # deadlocks instead of finishing.
        sys.stdout.flush()


if __name__ == "__main__":
    main()

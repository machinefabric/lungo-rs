"""The polyglot assertions, in Python, on the generated Python package."""

import asyncio
import random
import subprocess
import sys
import threading
import unittest

import lungo_py
import polyglot as P


class Scaler:
    """The scaler capability: multiplies by ten."""

    def host_scale(self, n):
        return n * 10


class Journal:
    """The journal capability: records lines, refusing `fail`."""

    def __init__(self):
        self.lines = []

    def host_record(self, line):
        if line == "fail":
            raise lungo_py.LeanIOError("the host refuses to record `fail`")
        self.lines.append(line)


JOURNAL = Journal()
P.set_scaler(Scaler())
P.set_journal(JOURNAL)


class Fetcher:
    """Answers the program's operations: `get` waits for `release` when the url is "slow", and
    raises for "broken"."""

    def __init__(self, release=None):
        self.release = release

    async def get(self, url):
        if url == "slow":
            await self.release.wait()
        elif url == "broken":
            raise ConnectionError("the network is down")
        elif url == "missing":
            return lungo_py.Err("not found")
        return lungo_py.Ok("body:" + url)

    def stamp(self, t):
        return t

    def scaler(self, k):
        return lambda x: x * k


def helper(code):
    """Runs `code` in a Python process of its own."""
    return subprocess.run([sys.executable, "-c", code], capture_output=True, text=True, timeout=120)


class PolyglotTest(unittest.TestCase):
    # TEST0048: numbers
    def test_0048_numbers(self):
        self.assertEqual(P.factorial(25), 15511210043330985984000000)
        self.assertEqual(P.negate(-123456789012345678901234567890), 123456789012345678901234567890)
        with self.assertRaises(lungo_py.MalformedError):
            P.factorial(-1)

    # TEST0049: structures and inductives
    def test_0049_structures_and_inductives(self):
        p = P.Point(1.5, -2.0, "p", 7)
        moved = P.move_by(p, 1.0, 0.5)
        self.assertEqual(moved, P.Point(2.5, -1.5, "p", 7))
        self.assertEqual(P.describe(moved), "p#7 at (2.500000, -1.500000)")
        self.assertEqual(P.area(P.ShapeRect(moved, 3.0, 4.0)), 12.0)
        self.assertEqual(P.area(P.ShapeCircle(p, 2.0)), 12.0)
        self.assertEqual(P.area(P.ShapeEmpty()), 0.0)
        with self.assertRaises(lungo_py.MalformedError):
            P.area(None)
        with self.assertRaises(lungo_py.MalformedError):
            P.move_by(P.Point(1.0, 2.0, "p", 256), 0.0, 0.0)

    # TEST0050: a function named like its type
    def test_0050_a_function_named_like_its_type(self):
        # `Lookup` the type and `Registry.lookup` the function: `Lookup` and `lookup`.
        self.assertEqual(P.lookup(3, 1), P.LookupFound(1))
        self.assertIsInstance(P.lookup(3, 5), P.LookupMissing)
        self.assertIsInstance(P.lookup(3, 5), P.Lookup)

    # TEST0051: scalars and containers
    def test_0051_scalars_and_containers(self):
        self.assertEqual(P.mix(2**64 - 1, -5, "λ", True), "18446744073709551615/-5/λ/true")
        self.assertEqual(P.reverse_bytes(b"\x01\x02\x03"), b"\x03\x02\x01")
        self.assertEqual(P.sum_floats([0.5, 0.25, 2.0]), 2.75)
        self.assertEqual(P.first_word("hello world"), "hello")
        self.assertIsNone(P.first_word(""))
        self.assertEqual(P.swap(("x", 9)), (9, "x"))
        self.assertEqual(P.evens(10), [0, 2, 4, 6, 8])
        self.assertEqual(P.divide(10, 0), lungo_py.Err("division by zero"))
        self.assertEqual(P.divide(10, 3), lungo_py.Ok(3))
        with self.assertRaises(lungo_py.MalformedError):
            P.first_word("\ud800")

    # TEST0052: errors
    def test_0052_errors(self):
        with self.assertRaises(lungo_py.LeanIOError) as e:
            P.checked_div(10, 0)
        self.assertEqual(e.exception.message, "checkedDiv: division by zero")
        with self.assertRaises(lungo_py.LeanError) as e:
            P.parse_digit("x")
        self.assertEqual(e.exception.value, "not a digit: x")
        self.assertEqual(P.parse_digit("7"), 7)

    # TEST0053: polymorphism
    def test_0053_polymorphism(self):
        tree = P.of_list(lungo_py.NAT, [1, 2, 3])
        self.assertEqual(P.size(lungo_py.NAT, tree), 3)
        mirrored = P.mirror(lungo_py.NAT, tree)
        self.assertIsInstance(mirrored, P.TreeNode)
        self.assertEqual(P.to_list(lungo_py.NAT, mirrored), [3, 2, 1])
        strings = P.TreeNode(P.TreeLeaf(), "s", P.TreeLeaf())
        self.assertEqual(P.size(lungo_py.STRING, strings), 1)
        with self.assertRaises(lungo_py.MalformedError):
            P.size(lungo_py.NAT, strings)

    # TEST0054: functions
    def test_0054_functions(self):
        calls = []

        def twice_plus_one(x):
            calls.append(x)
            return 2 * x + 1

        self.assertEqual(P.apply_twice(twice_plus_one, 3), 15)
        self.assertEqual(calls, [3, 7])
        self.assertEqual(P.adder(5, 3), 8)

    # TEST0055: opaque values
    def test_0055_opaque_values(self):
        counter = P.new_counter(10)
        self.assertEqual(P.bump(counter), 11)
        self.assertEqual(P.bump(counter), 12)
        counter.close()
        with self.assertRaises(lungo_py.MalformedError):
            P.bump(counter)

    # TEST0056: host capabilities
    def test_0056_host_capabilities(self):
        self.assertEqual(P.scaled_sum([1, 2, 3]), 60)
        JOURNAL.lines.clear()
        self.assertEqual(P.record_all(["a", "b"]), 2)
        with self.assertRaises(lungo_py.LeanIOError) as e:
            P.record_all(["c", "fail", "d"])
        self.assertIn("refuses to record", e.exception.message)
        self.assertEqual(JOURNAL.lines, ["a", "b", "c"])

    # TEST0057: concurrent calls
    def test_0057_concurrent_calls(self):
        results = {}

        def run(i):
            results[i] = P.factorial(i)

        threads = [threading.Thread(target=run, args=(i,)) for i in range(16)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        self.assertEqual(results[10], 3628800)
        self.assertEqual(len(results), 16)

    # TEST0058: run main
    def test_0058_run_main(self):
        self.assertEqual(P.run_main(["one", "two"]), 2)

    # TEST0297: a call before every capability is installed fails, naming the capability and an
    # operation of it
    def test_0297_a_missing_capability_is_named(self):
        out = helper(
            "import lungo_py, polyglot as P\n"
            "class Journal:\n"
            "    def host_record(self, line): pass\n"
            "P.set_journal(Journal())\n"
            "try:\n"
            "    P.factorial(3)\n"
            "except lungo_py.MissingCapabilityError as e:\n"
            "    print('missing:', e.capability, e.operation)\n"
        )
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertEqual(out.stdout, "missing: polyglot.scaler hostScale\n")

    # TEST0298: the package's assurance document is the program's: its claims, what they assume,
    # and the capabilities each export needs
    def test_0298_assurance_is_the_programs_assurance(self):
        a = P.ASSURANCE
        self.assertEqual((a.program, a.schema_version, a.provenance.lean_version), ("polyglot", 1, "4.34.1"))
        c = a.claim("Polyglot.factorial_pos")
        self.assertEqual((c.status, c.relation, c.assumptions), ("proved", "lungo.law", ()))
        self.assertEqual(a.claim("Polyglot.Tree.mirror_mirror").relation, "lungo.roundtrip")
        c = a.claim("Polyglot.divide_ok")
        self.assertEqual((c.relation, c.specifications), ("lungo.equals", ("Polyglot.natDiv",)))
        # Proved, and conditional on what the host's scaler is assumed to do.
        c = a.claim("Polyglot.scaledSum_singleton_mono")
        self.assertEqual((c.status, c.assumptions), ("proved", ("Polyglot.ScalesMonotonically",)))
        e = a.export("Polyglot.scaledSum")
        self.assertEqual((e.capabilities, e.assumptions), (("Polyglot.Scaler",), ("Polyglot.ScalesMonotonically",)))
        e = a.export("Polyglot.fetchAll")
        self.assertEqual((e.async_, e.capabilities), (True, ("Polyglot.fetchInterface",)))
        # Sorted by Lean name.
        self.assertEqual(
            [(c.id, c.form) for c in a.capabilities],
            [("polyglot.journal", "extern"), ("polyglot.scaler", "extern"), ("polyglot.fetch", "async")],
        )
        self.assertEqual(a.export("Polyglot.negate").claims, ())

    # TEST0299: an async program runs on its handler's answers: data, an opaque handle passed
    # back, and a host function the program calls
    def test_0299_an_async_program_runs_on_the_handlers_answers(self):
        async def run():
            h = Fetcher()
            self.assertEqual(await P.fetch_all(h, ["a", "missing", "b"]), ["body:a", "error: not found", "body:b"])
            token = P.mk_token(5)
            self.assertIsNotNone(token)
            self.assertEqual(await P.stamp_token(h, token), 6)
            self.assertEqual(await P.apply_scaler(h, 7, 6), 42)

        asyncio.run(run())
        self.assertEqual(lungo_py.outstanding(), 0)

    # TEST0300: an error of the handler abandons the program: the call raises it, and nothing
    # waits
    def test_0300_a_handlers_error_abandons_the_program(self):
        with self.assertRaisesRegex(ConnectionError, "the network is down"):
            asyncio.run(P.fetch_all(Fetcher(), ["a", "broken", "b"]))
        self.assertEqual(lungo_py.outstanding(), 0)

    # TEST0301: cancelling the task abandons a program waiting for an answer
    def test_0301_cancellation_abandons_the_program(self):
        async def run():
            task = asyncio.create_task(P.fetch_all(Fetcher(asyncio.Event()), ["slow"]))
            await asyncio.sleep(0.05)
            self.assertEqual(lungo_py.outstanding(), 1)
            task.cancel()
            with self.assertRaises(asyncio.CancelledError):
                await task

        asyncio.run(run())
        self.assertEqual(lungo_py.outstanding(), 0)

    # TEST0302: many async programs waiting at once resume independently, answered in any order
    def test_0302_concurrent_async_programs_resume_independently(self):
        async def run():
            release = asyncio.Event()
            h = Fetcher(release)

            async def one(i):
                await asyncio.sleep(random.random() / 50)
                return await P.fetch_all(h, ["slow", "x" * i])

            tasks = [asyncio.create_task(one(i)) for i in range(100)]
            await asyncio.sleep(0.1)
            self.assertEqual(lungo_py.outstanding(), 100)
            release.set()
            for i, got in enumerate(await asyncio.gather(*tasks)):
                self.assertEqual(got, ["body:slow", "body:" + "x" * i])

        asyncio.run(run())
        self.assertEqual(lungo_py.outstanding(), 0)

    # TEST0303: a process may end while a program waits for an answer
    def test_0303_a_process_ends_with_a_program_waiting(self):
        out = helper(
            "import asyncio, os, lungo_py, polyglot as P\n"
            "class Slow:\n"
            "    async def get(self, url):\n"
            "        await asyncio.Event().wait()\n"
            "async def main():\n"
            "    asyncio.create_task(P.fetch_all(Slow(), ['slow']))\n"
            "    while lungo_py.outstanding() == 0:\n"
            "        await asyncio.sleep(0.001)\n"
            "    print('waiting:', lungo_py.outstanding(), flush=True)\n"
            "    os._exit(0)\n"
            "class Host:\n"
            "    def host_scale(self, n): return n\n"
            "    def host_record(self, line): pass\n"
            "P.set_scaler(Host())\n"
            "P.set_journal(Host())\n"
            "asyncio.run(main())\n"
        )
        self.assertEqual((out.returncode, out.stdout), (0, "waiting: 1\n"), out.stderr)


if __name__ == "__main__":
    unittest.main()

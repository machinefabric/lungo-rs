"""The polyglot assertions, in Python, on the generated Python package."""

import threading
import unittest

import lungo_py
import polyglot as P


class Host:
    def __init__(self):
        self.lines = []

    def host_scale(self, n):
        return n * 10

    def host_record(self, line):
        if line == "fail":
            raise lungo_py.LeanIOError("the host refuses to record `fail`")
        self.lines.append(line)


HOST = Host()
P.set_host(HOST)


class PolyglotTest(unittest.TestCase):
    def test_numbers(self):
        self.assertEqual(P.factorial(25), 15511210043330985984000000)
        self.assertEqual(P.negate(-123456789012345678901234567890), 123456789012345678901234567890)
        with self.assertRaises(lungo_py.MalformedError):
            P.factorial(-1)

    def test_structures_and_inductives(self):
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

    def test_a_function_named_like_its_type(self):
        # `Lookup` the type and `Registry.lookup` the function: `Lookup` and `lookup`.
        self.assertEqual(P.lookup(3, 1), P.LookupFound(1))
        self.assertIsInstance(P.lookup(3, 5), P.LookupMissing)
        self.assertIsInstance(P.lookup(3, 5), P.Lookup)

    def test_scalars_and_containers(self):
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

    def test_errors(self):
        with self.assertRaises(lungo_py.LeanIOError) as e:
            P.checked_div(10, 0)
        self.assertEqual(e.exception.message, "checkedDiv: division by zero")
        with self.assertRaises(lungo_py.LeanError) as e:
            P.parse_digit("x")
        self.assertEqual(e.exception.value, "not a digit: x")
        self.assertEqual(P.parse_digit("7"), 7)

    def test_polymorphism(self):
        tree = P.of_list(lungo_py.NAT, [1, 2, 3])
        self.assertEqual(P.size(lungo_py.NAT, tree), 3)
        mirrored = P.mirror(lungo_py.NAT, tree)
        self.assertIsInstance(mirrored, P.TreeNode)
        self.assertEqual(P.to_list(lungo_py.NAT, mirrored), [3, 2, 1])
        strings = P.TreeNode(P.TreeLeaf(), "s", P.TreeLeaf())
        self.assertEqual(P.size(lungo_py.STRING, strings), 1)
        with self.assertRaises(lungo_py.MalformedError):
            P.size(lungo_py.NAT, strings)

    def test_functions(self):
        calls = []

        def twice_plus_one(x):
            calls.append(x)
            return 2 * x + 1

        self.assertEqual(P.apply_twice(twice_plus_one, 3), 15)
        self.assertEqual(calls, [3, 7])
        self.assertEqual(P.adder(5, 3), 8)

    def test_opaque_values(self):
        counter = P.new_counter(10)
        self.assertEqual(P.bump(counter), 11)
        self.assertEqual(P.bump(counter), 12)
        counter.close()
        with self.assertRaises(lungo_py.MalformedError):
            P.bump(counter)

    def test_host_externs(self):
        self.assertEqual(P.scaled_sum([1, 2, 3]), 60)
        HOST.lines.clear()
        self.assertEqual(P.record_all(["a", "b"]), 2)
        with self.assertRaises(lungo_py.LeanIOError) as e:
            P.record_all(["c", "fail", "d"])
        self.assertIn("refuses to record", e.exception.message)
        self.assertEqual(HOST.lines, ["a", "b", "c"])

    def test_concurrent_calls(self):
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

    def test_run_main(self):
        self.assertEqual(P.run_main(["one", "two"]), 2)


if __name__ == "__main__":
    unittest.main()

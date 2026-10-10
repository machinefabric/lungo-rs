"""The async-store example from Python: the store is a dict the handler answers from, in
coroutines."""

import asyncio
import unittest

import lungo_py
import store as S


class Memory:
    """A store in a dict, answering each operation after yielding to the event loop."""

    def __init__(self, entries):
        self.data = dict(entries)

    async def get(self, key):
        await asyncio.sleep(0)
        return self.data.get(key)

    async def put(self, key, value):
        await asyncio.sleep(0)
        if key == "readonly":
            raise PermissionError("the store is read-only there")
        self.data[key] = value


class StoreTest(unittest.TestCase):
    # TEST0339: the async store from Python
    def test_0339_the_async_store_from_python(self):
        async def run():
            m = Memory({"a": "1", "b": "2"})
            self.assertTrue(await S.copy(m, "a", "c"))
            self.assertTrue(await S.swap(m, "a", "b"))
            self.assertFalse(await S.swap(m, "a", "missing"))
            self.assertEqual(m.data, {"a": "2", "b": "1", "c": "1"})
            with self.assertRaises(PermissionError):
                await S.copy(m, "a", "readonly")

        asyncio.run(run())
        self.assertEqual(lungo_py.outstanding(), 0)
        claim = S.ASSURANCE.claim("Store.copy_model")
        self.assertEqual((claim.status, claim.specifications), ("proved", ("Store.Model",)))


if __name__ == "__main__":
    unittest.main()

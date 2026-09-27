# lungo-py

The support library of the Python packages [lungo](https://github.com/machinefabric/lungo)
generates from Lean programs: the lungo runtime, shared by every generated package of a process,
the wire format between Python and the runtime, and the Lean values Python has no type for.

Generated packages depend on exactly the `lungo-py` of the lungo release that generated them.

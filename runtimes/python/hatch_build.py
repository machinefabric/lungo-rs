"""Marks the wheel as platform-specific: it carries the lungo runtime built for one platform,
and no Python extension, so it serves every Python 3 (`py3-none-<platform>`)."""

import sysconfig

from hatchling.builders.hooks.plugin.interface import BuildHookInterface


class RuntimeWheel(BuildHookInterface):
    def initialize(self, version, build_data):
        if self.target_name != "wheel":
            return
        runtime = self.root + "/src/lungo_py/runtime"
        import os

        if not os.path.isdir(runtime):
            raise RuntimeError(
                "src/lungo_py/runtime is missing: lungo-py is built from a lungo distribution (`lungo-dist`)"
            )
        build_data["pure_python"] = False
        platform = sysconfig.get_platform().replace("-", "_").replace(".", "_")
        build_data["tag"] = f"py3-none-{platform}"

#!/usr/bin/env python3
"""Build the pinned pilot runtime without installing global Vulkan packages."""

import json
import shlex
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
from pathlib import Path


def is_headers_project(path):
    # The Android SDK can ship headers only, without the CMake project.
    return (path / "CMakeLists.txt").is_file() and (
        path / "include/spirv/unified1/spirv.hpp"
    ).is_file()


def self_test():
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory)
        header = path / "include/spirv/unified1/spirv.hpp"
        header.parent.mkdir(parents=True)
        header.touch()
        assert not is_headers_project(path), "headers alone must not enter cmake"
        (path / "CMakeLists.txt").touch()
        assert is_headers_project(path)
    print("SPIR-V project preflight passed")


def main():
    root = Path(__file__).resolve().parents[2] / "experiments/user2-pilot"
    manifest_path = root / "qwen-runtime.json"
    manifest = json.loads(manifest_path.read_text())
    source = root / "llama-src" / ("llama.cpp-" + manifest["llama_cpp_commit"])
    if not (source / "CMakeLists.txt").is_file():
        raise SystemExit("Pinned llama.cpp source must be prepared first")
    shader = Path("/usr/lib/android-sdk/ndk/28.2.13676358/shader-tools/linux-x86_64/glslc")
    if not shader.is_file():
        raise SystemExit("No local glslc; Vulkan GPU runtime cannot be built")
    dependency = root / "spirv-headers-source"
    if not dependency.exists():
        with urllib.request.urlopen(
            "https://api.github.com/repos/KhronosGroup/SPIRV-Headers/commits/main"
        ) as response:
            commit = json.load(response)["sha"]
        archive = root / "spirv-headers.tar.gz"
        urllib.request.urlretrieve(
            "https://codeload.github.com/KhronosGroup/SPIRV-Headers/tar.gz/" + commit,
            archive,
        )
        with tarfile.open(archive) as contents:
            contents.extractall(dependency, filter="data")
        manifest["spirv_headers_commit"] = commit
        manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
    headers = next(dependency.glob("SPIRV-Headers-*"), None)
    if headers is None or not is_headers_project(headers):
        raise SystemExit("Downloaded SPIR-V dependency is not a complete CMake project")
    prefix = root / "vulkan-deps"
    commands = [
        ["cmake", "-S", str(headers), "-B", str(root / "spirv-headers-build"),
         "-DCMAKE_INSTALL_PREFIX=" + str(prefix)],
        ["cmake", "--install", str(root / "spirv-headers-build")],
        ["c++", "-x", "c++", "-fsyntax-only", "-I" + str(prefix / "include"),
         "-include", "spirv/unified1/spirv.hpp", "/dev/null"],
        ["cmake", "-S", str(source), "-B", str(root / "llama-vulkan-build"),
         "-DCMAKE_PREFIX_PATH=" + str(prefix), "-DGGML_VULKAN=ON",
         # This pinned llama.cpp finds the package but omits its include target.
         "-DCMAKE_CXX_FLAGS=-I" + shlex.quote(str(prefix / "include")),
         "-DVulkan_GLSLC_EXECUTABLE=" + str(shader), "-DLLAMA_CURL=OFF",
         "-DLLAMA_BUILD_TESTS=OFF"],
        ["cmake", "--build", str(root / "llama-vulkan-build"), "-j", "4",
         "--target", "llama-server"],
    ]
    with (root / "llama-vulkan-build.log").open("a") as log:
        for command in commands:
            subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, check=True)
    print("Pinned native Vulkan runtime built; no global packages changed")


if __name__ == "__main__":
    if "--self-test" in sys.argv:
        self_test()
    else:
        main()

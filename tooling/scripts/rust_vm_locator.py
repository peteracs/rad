import os


def rust_vm_executable():
    """
    Locates the compiled Rust VM binary (rad / rad.exe).
    Prefers the debug build, falls back to release build.
    """
    repo_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    name = "rad.exe" if os.name == "nt" else "rad"

    debug_path = os.path.join(repo_root, "target", "debug", name)
    if os.path.isfile(debug_path):
        return debug_path

    release_path = os.path.join(repo_root, "target", "release", name)
    if os.path.isfile(release_path):
        return release_path

    return None

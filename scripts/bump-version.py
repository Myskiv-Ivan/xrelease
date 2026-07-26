#!/usr/bin/env python3
"""Keep xrelease version in sync across backend, frontend, OpenAPI, and Helm."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CARGO_TOML = ROOT / "Cargo.toml"
CARGO_LOCK = ROOT / "Cargo.lock"
OPENAPI = ROOT / "api" / "openapi.json"
PKG_JSON = ROOT / "front" / "package.json"
PKG_LOCK = ROOT / "front" / "package-lock.json"
CHART_YAML = ROOT / "deploy" / "helm" / "xrelease" / "Chart.yaml"
COMPOSE_YAML = ROOT / "docker-compose.yaml"
COMPOSE_DEV_YAML = ROOT / "docker" / "docker-compose.dev.yaml"

GHCR_BACKEND = "ghcr.io/myskiv-ivan/xrelease"
GHCR_UI = "ghcr.io/myskiv-ivan/xrelease-ui"
# Workspace members that each get their own [[package]] version stanza in Cargo.lock.
WORKSPACE_LOCK_PACKAGES = ("xrelease", "xrelease-cli")


def read_cargo_version() -> str:
    text = CARGO_TOML.read_text(encoding="utf-8")
    match = re.search(r'^version\s*=\s*"([^"]+)"', text, re.MULTILINE)
    if not match:
        raise SystemExit("Could not read version from Cargo.toml")
    return match.group(1)


def bump_semver(version: str, kind: str) -> str:
    parts = version.split(".")
    if len(parts) != 3 or not all(p.isdigit() for p in parts):
        raise SystemExit(f"Unsupported version format: {version!r} (expected X.Y.Z)")
    major, minor, patch = (int(p) for p in parts)
    if kind == "patch":
        patch += 1
    elif kind == "minor":
        minor += 1
        patch = 0
    elif kind == "major":
        major += 1
        minor = 0
        patch = 0
    else:
        raise SystemExit(f"Unknown bump kind: {kind}")
    return f"{major}.{minor}.{patch}"


def set_cargo_toml(version: str) -> None:
    text = CARGO_TOML.read_text(encoding="utf-8")
    updated, count = re.subn(
        r'^(version\s*=\s*")([^"]+)(")',
        rf"\g<1>{version}\3",
        text,
        count=1,
        flags=re.MULTILINE,
    )
    if count != 1:
        raise SystemExit("Failed to update Cargo.toml version")
    CARGO_TOML.write_text(updated, encoding="utf-8")


def set_cargo_lock(version: str) -> None:
    text = CARGO_LOCK.read_text(encoding="utf-8")
    for name in WORKSPACE_LOCK_PACKAGES:
        pattern = rf'(\[\[package\]\]\nname = "{re.escape(name)}"\nversion = ")([^"]+)(")'
        text, count = re.subn(pattern, rf"\g<1>{version}\3", text, count=1)
        if count != 1:
            raise SystemExit(f"Failed to update {name} version in Cargo.lock")
    CARGO_LOCK.write_text(text, encoding="utf-8")


def read_cargo_lock_versions() -> dict[str, str]:
    text = CARGO_LOCK.read_text(encoding="utf-8")
    versions: dict[str, str] = {}
    for name in WORKSPACE_LOCK_PACKAGES:
        match = re.search(
            rf'\[\[package\]\]\nname = "{re.escape(name)}"\nversion = "([^"]+)"',
            text,
        )
        versions[name] = match.group(1) if match else "?"
    return versions


def set_openapi(version: str) -> None:
    data = json.loads(OPENAPI.read_text(encoding="utf-8"))
    data.setdefault("info", {})["version"] = version
    OPENAPI.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def set_front_package(version: str) -> None:
    pkg = json.loads(PKG_JSON.read_text(encoding="utf-8"))
    pkg["version"] = version
    PKG_JSON.write_text(json.dumps(pkg, indent="\t") + "\n", encoding="utf-8")

    lock = json.loads(PKG_LOCK.read_text(encoding="utf-8"))
    lock["version"] = version
    root = lock.get("packages", {}).get("", {})
    if root:
        root["version"] = version
    PKG_LOCK.write_text(json.dumps(lock, indent="\t") + "\n", encoding="utf-8")


def set_helm_chart(version: str) -> None:
    text = CHART_YAML.read_text(encoding="utf-8")
    updated, n1 = re.subn(
        r'^(version:\s*)([^\s#]+)',
        rf"\g<1>{version}",
        text,
        count=1,
        flags=re.MULTILINE,
    )
    updated, n2 = re.subn(
        r'^(appVersion:\s*)("[^"]+"|[^\s#]+)',
        rf'\g<1>"{version}"',
        updated,
        count=1,
        flags=re.MULTILINE,
    )
    if n1 != 1 or n2 != 1:
        raise SystemExit("Failed to update Chart.yaml version/appVersion")
    CHART_YAML.write_text(updated, encoding="utf-8")


def _set_compose_file_images(path: Path, version: str) -> None:
    text = path.read_text(encoding="utf-8")
    updated, n1 = re.subn(
        rf"(image:\s*{re.escape(GHCR_BACKEND)}:)[\d.]+",
        rf"\g<1>{version}",
        text,
        count=1,
    )
    updated, n2 = re.subn(
        rf"(image:\s*{re.escape(GHCR_UI)}:)[\d.]+",
        rf"\g<1>{version}",
        updated,
        count=1,
    )
    if n1 != 1 or n2 != 1:
        raise SystemExit(f"Failed to update {path.relative_to(ROOT)} image tags")
    path.write_text(updated, encoding="utf-8")


def set_compose_images(version: str) -> None:
    _set_compose_file_images(COMPOSE_YAML, version)
    _set_compose_file_images(COMPOSE_DEV_YAML, version)


def _read_compose_file_versions(path: Path) -> tuple[str, str]:
    text = path.read_text(encoding="utf-8")
    backend = re.search(rf"image:\s*{re.escape(GHCR_BACKEND)}:([\d.]+)", text)
    ui = re.search(rf"image:\s*{re.escape(GHCR_UI)}:([\d.]+)", text)
    if not backend or not ui:
        raise SystemExit(f"Could not read image tags from {path.relative_to(ROOT)}")
    return backend.group(1), ui.group(1)


def read_compose_image_versions() -> tuple[str, str]:
    return _read_compose_file_versions(COMPOSE_YAML)


def read_compose_dev_image_versions() -> tuple[str, str]:
    return _read_compose_file_versions(COMPOSE_DEV_YAML)


def read_chart_versions() -> tuple[str, str]:
    text = CHART_YAML.read_text(encoding="utf-8")
    ver = re.search(r'^version:\s*([^\s#]+)', text, re.MULTILINE)
    app = re.search(r'^appVersion:\s*"?([^"\s#]+)"?', text, re.MULTILINE)
    if not ver or not app:
        raise SystemExit("Could not read version/appVersion from Chart.yaml")
    return ver.group(1), app.group(1)


def collect_versions() -> dict[str, str]:
    openapi = json.loads(OPENAPI.read_text(encoding="utf-8"))
    pkg = json.loads(PKG_JSON.read_text(encoding="utf-8"))
    lock = json.loads(PKG_LOCK.read_text(encoding="utf-8"))
    lock_root = lock.get("packages", {}).get("", {}).get("version")
    chart_ver, chart_app = read_chart_versions()
    compose_backend, compose_ui = read_compose_image_versions()
    compose_dev_backend, compose_dev_ui = read_compose_dev_image_versions()
    cargo_lock = read_cargo_lock_versions()

    versions = {
        "Cargo.toml": read_cargo_version(),
        "api/openapi.json": openapi.get("info", {}).get("version", "?"),
        "front/package.json": pkg.get("version", "?"),
        "front/package-lock.json": lock.get("version", "?"),
        "front/package-lock.json#root": lock_root or "?",
        "deploy/helm/.../Chart.yaml#version": chart_ver,
        "deploy/helm/.../Chart.yaml#appVersion": chart_app,
        "docker-compose.yaml#backend": compose_backend,
        "docker-compose.yaml#ui": compose_ui,
        "docker/docker-compose.dev.yaml#backend": compose_dev_backend,
        "docker/docker-compose.dev.yaml#ui": compose_dev_ui,
    }
    for name, ver in cargo_lock.items():
        versions[f"Cargo.lock#{name}"] = ver
    return versions


def check_sync() -> None:
    versions = collect_versions()
    unique = {v for v in versions.values()}
    if len(unique) == 1:
        print(f"Version sync OK: {next(iter(unique))}")
        for path, value in versions.items():
            print(f"  {path}: {value}")
        return

    print("Version mismatch:", file=sys.stderr)
    for path, value in versions.items():
        print(f"  {path}: {value}", file=sys.stderr)
    raise SystemExit(1)


def apply_version(version: str) -> None:
    set_cargo_toml(version)
    set_cargo_lock(version)
    set_openapi(version)
    set_front_package(version)
    set_helm_chart(version)
    set_compose_images(version)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Verify all version fields match")
    parser.add_argument("--bump", choices=["patch", "minor", "major"], help="Bump from Cargo.toml")
    parser.add_argument("--set", dest="set_version", metavar="X.Y.Z", help="Set explicit version")
    parser.add_argument(
        "--print",
        action="store_true",
        help="Print version only (with --bump/--set: after apply; alone: current Cargo.toml)",
    )
    args = parser.parse_args()

    if args.check:
        check_sync()
        return

    # `--print` alone: read-only current version (safe for commit messages / tagging).
    if args.print and not args.set_version and not args.bump:
        print(read_cargo_version())
        return

    if args.set_version:
        version = args.set_version
    elif args.bump:
        version = bump_semver(read_cargo_version(), args.bump)
    else:
        parser.error("one of --check, --bump, --set, or --print is required")

    apply_version(version)
    if args.print:
        print(version)
    else:
        print(f"Bumped to {version}")
        check_sync()


if __name__ == "__main__":
    main()

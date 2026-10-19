"""Release graph, archive integrity and staging-source regression checks."""
import hashlib
import io
import json
import subprocess
from pathlib import Path
import tarfile
import tempfile
import tomllib
import unittest
from unittest.mock import patch
from urllib.request import urlopen

import rust_release as release
import rust_registry as registry


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for workspace in ("framework-rs", "blueprint-rs"):
            directory = self.root / workspace
            (directory / "crates").mkdir(parents=True)
            (directory / "Cargo.toml").write_text('[workspace.package]\nversion="0.1.0"\nlicense="MIT"\nrepository="https://example.test/repo"\n')

    def crate(self, name, dependencies=None, **package):
        path = self.root / "framework-rs/crates" / name / "Cargo.toml"
        path.parent.mkdir(parents=True, exist_ok=True)
        manifest = {"package": {"name": name, "version": {"workspace": True}, "license": {"workspace": True},
                    "repository": {"workspace": True}, "description": "fixture", **package}, "dependencies": dependencies or {}}
        path.write_bytes(registry.manifest_text(manifest))
        return path

    def test_graph_versions_and_private_samples(self):
        self.crate("forklaunch-base")
        self.crate("forklaunch-app", {"base": {"package": "forklaunch-base", "path": "../forklaunch-base", "version": "=0.1.0"}})
        self.crate("forklaunch-sample", publish=False)
        self.assertEqual([p["name"] for p in release.release_plan(self.root)], ["forklaunch-base", "forklaunch-app"])

    def test_framework_default_excludes_blueprint_and_all_is_explicit(self):
        self.crate("forklaunch-base")
        blueprint = self.root / "blueprint-rs/crates/worker/Cargo.toml"
        blueprint.parent.mkdir()
        blueprint.write_text('[package]\nname="forklaunch-worker"\nversion="0.1.0"\nlicense="MIT"\nrepository="https://example.test/repo"\ndescription="worker"\n')
        self.assertEqual([p["name"] for p in release.release_plan(self.root)], ["forklaunch-base"])
        self.assertEqual([p["name"] for p in release.release_plan(self.root, "all")], ["forklaunch-base", "forklaunch-worker"])
        self.crate("forklaunch-base", {"forklaunch-worker": {"path": "../../../blueprint-rs/crates/worker", "version": "0.1.0"}})
        with self.assertRaisesRegex(ValueError, "outside framework release scope"):
            release.release_plan(self.root)

    def test_rejects_unpublishable_dependencies(self):
        self.crate("forklaunch-base")
        invalid = [
            {"path": "../forklaunch-base"},
            {"path": "../forklaunch-base", "version": "0.2.0"},
            {"path": "../missing", "version": "0.1.0"},
            {"git": "https://example.test/dependency"},
        ]
        for dependency in invalid:
            with self.subTest(dependency=dependency):
                self.crate("forklaunch-app", {"forklaunch-base": dependency})
                with self.assertRaises(ValueError):
                    release.release_plan(self.root)
        self.crate("forklaunch-base", publish=False)
        self.crate("forklaunch-app", {"forklaunch-base": {"path": "../forklaunch-base", "version": "0.1.0"}})
        with self.assertRaisesRegex(ValueError, "private sample"):
            release.release_plan(self.root)

    def test_dev_dependency_backedge_does_not_create_publication_cycle(self):
        macro = self.crate("forklaunch-macros")
        self.crate("forklaunch-core", {"forklaunch-macros": {"path": "../forklaunch-macros", "version": "0.1.0"}})
        manifest = tomllib.loads(macro.read_text())
        manifest["dev-dependencies"] = {"forklaunch-core": {"path": "../forklaunch-core", "version": "0.1.0"}}
        macro.write_bytes(registry.manifest_text(manifest))
        self.assertEqual([p["name"] for p in release.release_plan(self.root)],
                         ["forklaunch-macros", "forklaunch-core"])

    def test_cycle_fails(self):
        self.crate("forklaunch-a", {"forklaunch-b": {"path": "../forklaunch-b", "version": "0.1.0"}})
        self.crate("forklaunch-b", {"forklaunch-a": {"path": "../forklaunch-a", "version": "0.1.0"}})
        with self.assertRaisesRegex(ValueError, "cycle"):
            release.release_plan(self.root)

    def archive(self, manifest, extra=None):
        buffer = io.BytesIO()
        with tarfile.open(fileobj=buffer, mode="w:gz") as archive:
            for name, data in {"forklaunch-a-0.1.0/Cargo.toml": registry.manifest_text(manifest),
                               "forklaunch-a-0.1.0/src/lib.rs": b"pub fn answer() -> u8 { 42 }", **(extra or {})}.items():
                member = tarfile.TarInfo(name)
                member.size = len(data)
                archive.addfile(member, io.BytesIO(data))
        return buffer.getvalue()

    def manifest(self):
        return {"package": {"name": "forklaunch-a", "version": "0.1.0", "edition": "2024", "rust-version": "1.85"},
                "features": {"base": ["dep:owned", "owned?/extra"]},
                "dependencies": {"owned": {"package": "forklaunch-b", "version": "0.1", "optional": True},
                                 "serde": {"version": "1", "default-features": False, "features": ["derive"]}},
                "target": {"cfg(unix)": {"build-dependencies": {"cc": "1"}}}}

    def test_stage_preserves_source_and_dependency_semantics(self):
        data, index = registry.stage_package(self.archive(self.manifest()), {"name": "forklaunch-a", "version": "0.1.0"}, {"forklaunch-a", "forklaunch-b"})
        self.assertEqual(index["rust_version"], "1.85")
        self.assertEqual(index["features2"], self.manifest()["features"])
        owned, serde, cc = index["deps"]
        self.assertEqual((owned["name"], owned["package"], owned["registry"], owned["optional"]), ("owned", "forklaunch-b", None, True))
        self.assertEqual((serde["registry"], serde["default_features"]), (release.REGISTRY, False))
        self.assertEqual((cc["kind"], cc["target"]), ("build", "cfg(unix)"))
        with tarfile.open(fileobj=io.BytesIO(data)) as archive:
            self.assertEqual(archive.extractfile("forklaunch-a-0.1.0/src/lib.rs").read(), b"pub fn answer() -> u8 { 42 }")
            manifest = tomllib.loads(archive.extractfile("forklaunch-a-0.1.0/Cargo.toml").read().decode())
        self.assertEqual(manifest["dependencies"]["owned"]["registry"], "forklaunch-staging")
        self.assertNotIn("registry", manifest["dependencies"]["serde"])

    def test_archive_path_and_source_rejections(self):
        for member in ("../escape", "/escape", "other-root/file"):
            with self.subTest(member=member), self.assertRaises(ValueError):
                registry.stage_package(self.archive(self.manifest(), {member: b"bad"}), {"name": "forklaunch-a", "version": "0.1.0"}, set())
        for source in ("path", "git", "registry-index", "registry"):
            manifest = self.manifest()
            manifest["dependencies"]["serde"][source] = "unexpected"
            with self.subTest(source=source), self.assertRaises(ValueError):
                registry.stage_package(self.archive(manifest), {"name": "forklaunch-a", "version": "0.1.0"}, set())

    def test_checksum_and_live_sparse_index(self):
        contents = self.archive(self.manifest())
        (self.root / "package.crate").write_bytes(contents)
        package = {"name": "forklaunch-a", "version": "0.1.0", "archive": "package.crate", "sha256": hashlib.sha256(contents).hexdigest()}
        with registry.registry(self.root, {"packages": [package]}) as index:
            base = index.removeprefix("sparse+")
            with urlopen(base + "config.json") as response:
                self.assertIn("{crate}", json.load(response)["dl"])
            with urlopen(base + "fo/rk/forklaunch-a") as response:
                entry = json.load(response)
            with urlopen(base + "api/v1/crates/forklaunch-a/0.1.0/download") as response:
                self.assertEqual(hashlib.sha256(response.read()).hexdigest(), entry["cksum"])
        with self.assertRaisesRegex(ValueError, "checksum"):
            registry.checked_archive(self.root, {**package, "sha256": "wrong"})
        with self.assertRaisesRegex(ValueError, "escapes"):
            registry.checked_archive(self.root, {**package, "archive": "../outside"})

    def test_publish_defaults_to_no_upload(self):
        report = {"packages": [{"name": "forklaunch-a", "version": "0.1.0", "manifest": "framework-rs/crates/forklaunch-a/Cargo.toml"}]}
        with patch.object(release, "publication_preflight", return_value=report), patch.object(release, "run") as run:
            release.publish(self.root, "rust-v0.1.0", False)
            run.assert_not_called()
        (self.root / "release.json").write_text(json.dumps(report))
        with patch.object(release, "publication_preflight", return_value=report), patch.dict(release.os.environ, {}, clear=True), patch.object(release, "run") as run, patch.object(release, "published_version", return_value=None), patch.object(release, "wait_for_version", return_value={}), patch.object(release, "verify_published_archive", return_value="checked"):
            release.publish(self.root, "rust-v0.1.0", True)
            run.assert_called_once()  # Cargo's credential provider resolves cargo-login credentials.
        progress = json.loads((self.root / "publication.json").read_text())
        self.assertTrue(progress["complete"])
        self.assertFalse(progress["packages"][0]["reusedMatchingVersion"])

    def test_resume_only_reuses_verified_existing_archives(self):
        report = {"packages": [{"name": "forklaunch-a", "version": "0.1.0", "manifest": "framework-rs/crates/forklaunch-a/Cargo.toml"}]}
        (self.root / "release.json").write_text(json.dumps(report))
        with patch.object(release, "publication_preflight", return_value=report), patch.object(release, "run") as run, patch.object(release, "published_version", return_value={"cksum": "checked"}), patch.object(release, "verify_published_archive", return_value="checked") as verify:
            release.publish(self.root, "rust-v0.1.0", True)
            run.assert_not_called()
            verify.assert_called_once()
        progress = json.loads((self.root / "publication.json").read_text())
        self.assertTrue(progress["packages"][0]["reusedMatchingVersion"])
        with patch.object(release, "publication_preflight", return_value=report), patch.object(release, "run") as run, patch.object(release, "published_version", return_value={}), patch.object(release, "verify_published_archive", side_effect=ValueError("different sources")):
            with self.assertRaisesRegex(ValueError, "different sources"):
                release.publish(self.root, "rust-v0.1.0", True)
            run.assert_not_called()

    def test_registry_archive_allows_only_generated_lockfile_difference(self):
        original = self.archive(self.manifest())
        (self.root / "a.crate").write_bytes(original)
        package = {"name": "forklaunch-a", "version": "0.1.0", "archive": "a.crate", "sha256": hashlib.sha256(original).hexdigest()}
        published = self.archive(self.manifest(), {"forklaunch-a-0.1.0/Cargo.lock": b"generated lock"})
        checksum = hashlib.sha256(published).hexdigest()
        with patch.object(release, "urlopen", return_value=io.BytesIO(published)):
            self.assertEqual(release.verify_published_archive(self.root, package, {"cksum": checksum}), checksum)
        changed = self.archive(self.manifest(), {"forklaunch-a-0.1.0/src/lib.rs": b"different source"})
        with patch.object(release, "urlopen", return_value=io.BytesIO(changed)):
            with self.assertRaisesRegex(ValueError, "differs from verified"):
                release.verify_published_archive(self.root, package, {"cksum": hashlib.sha256(changed).hexdigest()})

    def test_registry_wait_is_bounded(self):
        with patch.object(release, "published_version", return_value=None), patch.object(release.time, "monotonic", side_effect=[0, 181]):
            with self.assertRaisesRegex(ValueError, "rerun to resume"):
                release.wait_for_version({"name": "forklaunch-a", "version": "0.1.0"})

    def test_dirty_or_unverified_release_cannot_publish(self):
        for report in ({"schemaVersion": 1, "dirty": True}, {"schemaVersion": 1, "dirty": False, "packages": []}):
            (self.root / "release.json").write_text(json.dumps(report))
            (self.root / "verification.json").write_text("{}")
            with self.assertRaises(ValueError):
                release.publication_preflight(self.root, "rust-v0.1.0")

    def test_regex_release_requires_optional_feature_proof(self):
        report = {"schemaVersion": 1, "dirty": False,
                  "packages": [{"name": "forklaunch-regress"}]}
        contents = json.dumps(report).encode()
        (self.root / "release.json").write_bytes(contents)
        for proof in (None, False, "true"):
            with self.subTest(proof=proof):
                (self.root / "verification.json").write_text(json.dumps({
                    "schemaVersion": 1, "httpMcpStartup": True, "packages": 1,
                    "releaseSha256": hashlib.sha256(contents).hexdigest(),
                    "boundedRegex": proof,
                }))
                with self.assertRaisesRegex(ValueError, "bounded regex"):
                    release.publication_preflight(self.root, "rust-v0.1.0")

    def test_native_worker_release_requires_optional_feature_proof(self):
        report = {"schemaVersion": 1, "dirty": False,
                  "packages": [{"name": "forklaunch-worker-interface"}]}
        contents = json.dumps(report).encode()
        (self.root / "release.json").write_bytes(contents)
        for proof in (None, False, "true"):
            with self.subTest(proof=proof):
                (self.root / "verification.json").write_text(json.dumps({
                    "schemaVersion": 1, "httpMcpStartup": True, "packages": 1,
                    "releaseSha256": hashlib.sha256(contents).hexdigest(),
                    "nativeHttpWorkerFeatures": proof,
                }))
                with self.assertRaisesRegex(ValueError, "native HTTP/worker"):
                    release.publication_preflight(self.root, "rust-v0.1.0")

    def test_publication_binds_clean_tag_graph_and_artifacts(self):
        self.crate("forklaunch-a")
        def git(*args):
            return subprocess.check_output(["git", *args], cwd=self.root, stderr=subprocess.DEVNULL, text=True).strip()
        git("init")
        git("add", ".")
        git("-c", "user.name=Release Fixture", "-c", "user.email=release@example.test", "commit", "-m", "fixture")
        commit = git("rev-parse", "HEAD")
        git("tag", "rust-v0.1.0")
        with tempfile.TemporaryDirectory() as artifacts:
            output = Path(artifacts)
            (output / "package.crate").write_bytes(b"fixture archive")
            artifact = {"archive": "package.crate", "sha256": hashlib.sha256(b"fixture archive").hexdigest()}
            report = {"schemaVersion": 1, "scope": "framework", "commit": commit, "dirty": False,
                      "packages": [{**release.release_plan(self.root)[0], **artifact}], "requiredRootPatch": artifact}
            def save():
                contents = json.dumps(report).encode()
                (output / "release.json").write_bytes(contents)
                (output / "verification.json").write_text(json.dumps({"schemaVersion": 1, "httpMcpStartup": True,
                    "releaseSha256": hashlib.sha256(contents).hexdigest(), "packages": 1}))
            save()
            with patch.object(release, "ROOT", self.root), patch.object(release, "release_plan", side_effect=lambda **kwargs: original_plan(self.root, **kwargs)):
                self.assertEqual(release.publication_preflight(output, "rust-v0.1.0"), report)
                report["scope"] = "all"
                save()
                with self.assertRaisesRegex(ValueError, "Only framework"):
                    release.publication_preflight(output, "rust-v0.1.0")
                report["scope"] = "framework"
                report["packages"][0]["manifest"] = "blueprint-rs/crates/forklaunch-a/Cargo.toml"
                save()
                with self.assertRaisesRegex(ValueError, "Only framework"):
                    release.publication_preflight(output, "rust-v0.1.0")
                report["packages"][0]["manifest"] = "framework-rs/crates/forklaunch-a/Cargo.toml"
                save()
                with self.assertRaisesRegex(ValueError, "tag"):
                    release.publication_preflight(output, "rust-v9.0.0")
                (self.root / "untracked").write_text("dirty")
                with self.assertRaisesRegex(ValueError, "clean checkout"):
                    release.publication_preflight(output, "rust-v0.1.0")
                (self.root / "untracked").unlink()
                report["commit"] = "0" * 40
                save()
                with self.assertRaisesRegex(ValueError, "same commit"):
                    release.publication_preflight(output, "rust-v0.1.0")
                report["commit"] = commit
                report["packages"][0]["version"] = "9.0.0"
                save()
                with self.assertRaisesRegex(ValueError, "inventory"):
                    release.publication_preflight(output, "rust-v0.1.0")
                report["packages"][0]["version"] = "0.1.0"
                save()
                (output / "package.crate").write_bytes(b"tampered")
                with self.assertRaisesRegex(ValueError, "checksum"):
                    release.publication_preflight(output, "rust-v0.1.0")


original_plan = release.release_plan


if __name__ == "__main__":
    unittest.main()

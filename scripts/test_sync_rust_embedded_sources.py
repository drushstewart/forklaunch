"""Source catalog safety regressions; no repository files are mutated."""
import tempfile
import unittest
from pathlib import Path
from sync_rust_embedded_sources import sync_tree


class CatalogTests(unittest.TestCase):
    def test_git_metadata_is_regular_owned_copy_and_updates(self):
        for name in ['.gitignore', '.gitattributes']:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                source, catalog = root / 'source', root / 'catalog'
                source.mkdir()
                catalog.mkdir()
                original, embedded = source / name, catalog / name
                original.write_text('original\n')
                embedded.symlink_to(original)
                sync_tree(source, catalog)
                self.assertFalse(embedded.is_symlink())
                self.assertEqual(embedded.read_bytes(), original.read_bytes())
                modified = embedded.stat().st_mtime_ns
                sync_tree(source, catalog)
                self.assertEqual(embedded.stat().st_mtime_ns, modified)
                original.write_text('updated\n')
                sync_tree(source, catalog)
                self.assertEqual(embedded.read_bytes(), original.read_bytes())

    def test_unexpected_git_metadata_link_is_not_replaced(self):
        for name in ['.gitignore', '.gitattributes']:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                source, catalog = root / 'source', root / 'catalog'
                source.mkdir()
                catalog.mkdir()
                (source / name).write_text('original\n')
                other = root / 'user-owned'
                other.write_text('preserve\n')
                embedded = catalog / name
                embedded.symlink_to(other)
                with self.assertRaises(ValueError):
                    sync_tree(source, catalog)
                self.assertTrue(embedded.is_symlink())
                self.assertEqual(embedded.resolve(), other.resolve())
                self.assertEqual(other.read_text(), 'preserve\n')

    def test_owned_directory_link_becomes_filtered_file_links(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / 'source'
            source.mkdir()
            for name in ['src/build/mod.rs', '__pycache__/test.pyc', 'nested/cache.pyc',
                         'node_modules/pkg/index.js', '.env.production', '.env.example',
                         'Cargo.toml', 'Cargo.lock']:
                path = source / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('original')
            catalog = root / 'catalog'
            catalog.symlink_to(source, target_is_directory=True)
            sync_tree(source, catalog)
            self.assertFalse(catalog.is_symlink())
            self.assertFalse((catalog / 'src').is_symlink())
            self.assertTrue((catalog / 'src/build/mod.rs').is_symlink())
            self.assertTrue((catalog / 'Cargo.toml.template').is_file())
            self.assertTrue((catalog / 'Cargo.lock.template').is_file())
            self.assertTrue((catalog / '.env.example').is_file())
            self.assertFalse((catalog / '.env.production').exists())
            self.assertFalse((catalog / '__pycache__').exists())
            self.assertFalse((catalog / 'nested/cache.pyc').exists())
            self.assertFalse((catalog / 'node_modules').exists())
            self.assertEqual((source / '.env.production').read_text(), 'original')
            self.assertEqual((source / '__pycache__/test.pyc').read_text(), 'original')
            sync_tree(source, catalog)  # Idempotent and live-linked.
            (source / 'src/build/mod.rs').write_text('updated')
            self.assertEqual((catalog / 'src/build/mod.rs').read_text(), 'updated')

    def test_unknown_catalog_entries_are_preserved_and_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, catalog = root / 'source', root / 'catalog'
            source.mkdir()
            catalog.mkdir()
            unknown = catalog / 'custom.rs'
            unknown.write_text('user owned')
            with self.assertRaises(ValueError):
                sync_tree(source, catalog)
            self.assertEqual(unknown.read_text(), 'user owned')

    def test_unexpected_directory_link_is_not_replaced(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, other = root / 'source', root / 'other'
            source.mkdir()
            other.mkdir()
            catalog = root / 'catalog'
            catalog.symlink_to(other, target_is_directory=True)
            with self.assertRaises(ValueError):
                sync_tree(source, catalog)
            self.assertEqual(catalog.resolve(), other.resolve())


if __name__ == '__main__':
    unittest.main()

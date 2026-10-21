"""Embedded ignore-file safety regressions; no repository files are mutated."""
import tempfile
import unittest
from pathlib import Path
from sync_rust_embedded_sources import mirror_git_metadata


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
                mirror_git_metadata(original, embedded)
                self.assertFalse(embedded.is_symlink())
                self.assertEqual(embedded.read_bytes(), original.read_bytes())
                modified = embedded.stat().st_mtime_ns
                mirror_git_metadata(original, embedded)
                self.assertEqual(embedded.stat().st_mtime_ns, modified)
                original.write_text('updated\n')
                mirror_git_metadata(original, embedded)
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
                    mirror_git_metadata(source / name, embedded)
                self.assertTrue(embedded.is_symlink())
                self.assertEqual(embedded.resolve(), other.resolve())
                self.assertEqual(other.read_text(), 'preserve\n')


if __name__ == '__main__':
    unittest.main()

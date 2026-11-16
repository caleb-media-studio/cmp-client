import unittest
import json
from unittest.mock import patch, MagicMock
from urllib.error import HTTPError
from io import BytesIO
from cmp_context import CMPClient, CMPError

class TestClient(unittest.TestCase):
    def test_endpoint_and_token(self):
        c = CMPClient("http://127.0.0.1:8787/", "sample")
        fake = MagicMock()
        fake.__enter__.return_value.read.return_value = b'{"repositories":["demo"]}'
        with patch("cmp_context.urlopen", return_value=fake) as call:
            result = c.list_repositories()
            self.assertEqual(result["repositories"], ["demo"])
            self.assertEqual(call.call_args[0][0].get_header("Authorization"), "Bearer sample")

    def test_error_mapping(self):
        c = CMPClient("http://127.0.0.1:8787", "sample")
        error = HTTPError("http://example", 409, "Conflict", {}, BytesIO(b'{"error":"conflict"}'))
        with patch("cmp_context.urlopen", side_effect=error):
            with self.assertRaises(CMPError):
                c.head("demo")

if __name__ == "__main__":
    unittest.main()

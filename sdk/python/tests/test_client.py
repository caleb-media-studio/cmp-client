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


    def test_cmp_transport_url_validation(self):
        from cmp_context import cmp_transport_path
        self.assertEqual(cmp_transport_path("team/knowledge"), "/team/knowledge.cmp")
        for candidate in ("team/../bad", "team/repo/extra", "team/..", "/repo"):
            with self.assertRaises(CMPError):
                cmp_transport_path(candidate)

    def test_cmp_transport_uses_no_redirect_opener(self):
        c = CMPClient("https://cloud.cmp.chat", "test-token")
        fake = MagicMock()
        fake.open.return_value.__enter__.return_value.read.return_value = (
            b'{"protocol":"cmp","transportVersion":1}')
        with patch("cmp_context.build_opener", return_value=fake) as opener:
            result = c.cmp_manifest("team/knowledge")
            self.assertEqual(result["protocol"], "cmp")
            req = fake.open.call_args.args[0]
            self.assertEqual(req.full_url, "https://cloud.cmp.chat/team/knowledge.cmp")
            self.assertEqual(req.get_header("Authorization"), "Bearer test-token")
            self.assertEqual(type(opener.call_args.args[0]).__name__, "_RejectTransportRedirects")

if __name__ == "__main__":
    unittest.main()

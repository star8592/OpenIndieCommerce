import importlib.util
import pathlib
import unittest

P = pathlib.Path(__file__).parents[1] / "provider_acceptance.py"
spec = importlib.util.spec_from_file_location("provider_acceptance", P)
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class ProviderAcceptanceTests(unittest.TestCase):
    def test_provider_map(self):
        rows = m.provider_map(
            {"providers": [{"provider": "dodo", "configured": True, "mode": "test"}]}
        )
        self.assertTrue(rows["dodo"]["configured"])
        self.assertEqual(rows["dodo"]["mode"], "test")

    def test_readiness_report_all_ready(self):
        status = {
            "paddle": {"configured": True, "mode": None},
            "dodo": {"configured": True, "mode": "test"},
            "zpay": {"configured": True, "mode": None},
        }
        report = m.readiness_report(status, m.PROVIDERS)
        self.assertEqual(report["status"], "ALL_PROVIDER_CONFIG_READY")
        self.assertEqual(len(report["providers"]), 3)

    def test_readiness_report_partial(self):
        status = {
            "paddle": {"configured": True, "mode": None},
            "dodo": {"configured": False, "mode": "test"},
        }
        report = m.readiness_report(status, m.PROVIDERS)
        self.assertEqual(report["status"], "PARTIAL")
        self.assertFalse(report["providers"][2]["configured"])

    def test_extracts_and_unescapes_dodo_checkout_url(self):
        page = '<a href="https://test.dodopayments.com/a?x=1&amp;y=2">Continue</a>'
        self.assertEqual(
            m.dodo_checkout_url(page),
            "https://test.dodopayments.com/a?x=1&y=2",
        )

    def test_missing_dodo_link_fails_closed(self):
        with self.assertRaises(m.AcceptanceError):
            m.dodo_checkout_url("<p>missing</p>")

    def test_paddle_checkout_page_markers(self):
        m.assert_checkout_page(
            "paddle",
            "<button>Continue to secure checkout</button><script>Paddle.Checkout.open()</script>",
        )

    def test_zpay_checkout_page_markers(self):
        m.assert_checkout_page("zpay", "<button>支付宝</button><button>微信支付</button>")

    def test_missing_checkout_marker_fails_closed(self):
        with self.assertRaises(m.AcceptanceError):
            m.assert_checkout_page("paddle", "<p>not a checkout</p>")

    def test_default_currency(self):
        self.assertEqual(m.default_currency("zpay"), "CNY")
        self.assertEqual(m.default_currency("paddle"), "USD")
        self.assertEqual(m.default_currency("dodo"), "USD")


if __name__ == "__main__":
    unittest.main()

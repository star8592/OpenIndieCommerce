import importlib.util, pathlib, unittest
P = pathlib.Path(__file__).parents[1] / 'provider_acceptance.py'
spec = importlib.util.spec_from_file_location('provider_acceptance', P)
m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
class ProviderAcceptanceTests(unittest.TestCase):
    def test_provider_map(self):
        rows = m.provider_map({'providers':[{'provider':'dodo','configured':True,'mode':'test'}]})
        self.assertTrue(rows['dodo']['configured']); self.assertEqual(rows['dodo']['mode'],'test')
    def test_extracts_and_unescapes_checkout_url(self):
        page='<a href="https://test.dodopayments.com/a?x=1&amp;y=2">Continue</a>'
        self.assertEqual(m.dodo_checkout_url(page),'https://test.dodopayments.com/a?x=1&y=2')
    def test_missing_link_fails_closed(self):
        with self.assertRaises(m.AcceptanceError): m.dodo_checkout_url('<p>missing</p>')
if __name__ == '__main__': unittest.main()

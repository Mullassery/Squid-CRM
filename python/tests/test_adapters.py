import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from squid_crm import person_activation_payload, to_pycustomerhub_event  # noqa: E402


class AdapterTests(unittest.TestCase):
    def envelope(self):
        return {
            "id": "event-1",
            "type": "person.created",
            "schemaVersion": "1.0",
            "workspaceId": "workspace-1",
            "occurredAt": "2026-01-01T00:00:00Z",
            "data": {"id": "person-1"},
        }

    def test_maps_documented_top_level_fields(self):
        mapped = to_pycustomerhub_event(
            self.envelope(),
            identity={"anonymous_id": "person-1"},
            schema="crm.person.v1",
            tenant_id="workspace-1",
        )
        self.assertEqual(mapped["event_id"], "event-1")
        self.assertEqual(mapped["event_type"], "person.created")
        self.assertEqual(mapped["timestamp"], "2026-01-01T00:00:00Z")
        self.assertEqual(mapped["tenant_id"], "workspace-1")
        self.assertEqual(mapped["schema"], "crm.person.v1")
        self.assertEqual(mapped["correlation_id"], "event-1")
        self.assertEqual(mapped["payload"], {"id": "person-1"})

    def test_requires_identity_and_core_fields(self):
        with self.assertRaises(ValueError):
            to_pycustomerhub_event(
                self.envelope(), identity={}, schema="s", tenant_id="t"
            )
        broken = self.envelope()
        del broken["data"]
        with self.assertRaises(ValueError):
            to_pycustomerhub_event(
                broken, identity={"id": "p"}, schema="s", tenant_id="t"
            )

    def test_correlation_id_can_be_supplied(self):
        mapped = to_pycustomerhub_event(
            self.envelope(),
            identity={"id": "p"},
            schema="s",
            tenant_id="t",
            correlation_id="corr-1",
        )
        self.assertEqual(mapped["correlation_id"], "corr-1")


    def test_person_activation_payload_uses_allowlist(self):
        payload = person_activation_payload(
            {
                "email": "ada@example.com",
                "first_name": "Ada",
                "last_name": "Lovelace",
                "job_title": "Engineer",
                "company_id": "company-1",
                "ssn": "never-forward-this",
            }
        )
        self.assertEqual(payload["emails"]["primaryEmail"], "ada@example.com")
        self.assertEqual(payload["name"], {"firstName": "Ada", "lastName": "Lovelace"})
        self.assertEqual(payload["jobTitle"], "Engineer")
        self.assertEqual(payload["companyId"], "company-1")
        self.assertNotIn("ssn", payload)

    def test_person_activation_payload_requires_email(self):
        with self.assertRaises(ValueError):
            person_activation_payload({"first_name": "Ada"})

    def test_person_activation_payload_supports_field_map(self):
        payload = person_activation_payload(
            {"work_email": "grace@example.com", "given_name": "Grace"},
            field_map={"email": "work_email", "first_name": "given_name"},
        )
        self.assertEqual(payload["emails"]["primaryEmail"], "grace@example.com")
        self.assertEqual(payload["name"]["firstName"], "Grace")


if __name__ == "__main__":
    unittest.main()

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from squid_crm import person_activation_payload, to_pycustomerhub_event  # noqa: E402

EVENT_ID = "11111111-1111-4111-8111-111111111111"
TENANT_ID = "22222222-2222-4222-8222-222222222222"
WORKSPACE_ID = "33333333-3333-4333-8333-333333333333"


class AdapterTests(unittest.TestCase):
    def envelope(self):
        return {
            "id": EVENT_ID,
            "type": "person.created",
            "schemaVersion": "1.0",
            "workspaceId": WORKSPACE_ID,
            "occurredAt": "2026-01-01T00:00:00Z",
            "data": {"id": "person-1", "emails": {"primaryEmail": "ada@example.com"}},
        }

    def test_maps_to_canonical_ocds_envelope(self):
        mapped = to_pycustomerhub_event(
            self.envelope(),
            identity={"crm_id": "person-1", "email": "ada@example.com"},
            schema="squid.person@1.0",
            tenant_id=TENANT_ID,
        )
        self.assertEqual(mapped["event_id"], EVENT_ID)
        self.assertEqual(mapped["event_type"], "person.created")
        self.assertEqual(mapped["timestamp"], "2026-01-01T00:00:00Z")
        self.assertEqual(mapped["tenant_id"], TENANT_ID)
        self.assertEqual(mapped["schema"], {"name": "squid.person", "version": "1.0"})
        self.assertEqual(mapped["source"], {"type": "squid-crm", "name": "squid-webhook"})
        self.assertEqual(mapped["correlation_id"], EVENT_ID)
        self.assertEqual(mapped["data"], self.envelope()["data"])
        self.assertEqual(mapped["context"]["squid"]["object"], "person")
        self.assertEqual(mapped["context"]["squid"]["operation"], "created")
        self.assertEqual(mapped["context"]["squid"]["workspace_id"], WORKSPACE_ID)
        self.assertNotIn("payload", mapped)

        identities = {ref["namespace"]: ref for ref in mapped["identity"]}
        self.assertEqual(identities["crm_id"]["value"], "person-1")
        self.assertTrue(identities["crm_id"]["primary"])
        self.assertTrue(all(ref["source"] == "squid-crm" for ref in identities.values()))

    def test_accepts_single_claim_and_preserves_consent(self):
        envelope = self.envelope()
        envelope["consent"] = {"analytics": True}
        mapped = to_pycustomerhub_event(
            envelope,
            identity={"namespace": "email", "value": "ada@example.com", "primary": True},
            schema="crm.person.v1",
            tenant_id=TENANT_ID,
        )
        self.assertEqual(mapped["identity"][0]["namespace"], "email")
        self.assertTrue(mapped["identity"][0]["primary"])
        self.assertEqual(mapped["schema"], {"name": "crm.person", "version": "v1"})
        self.assertEqual(mapped["consent"], {"analytics": True})

    def test_bare_schema_uses_envelope_version(self):
        mapped = to_pycustomerhub_event(
            self.envelope(),
            identity={"id": "p"},
            schema="crm.person",
            tenant_id=TENANT_ID,
        )
        self.assertEqual(mapped["schema"], {"name": "crm.person", "version": "1.0"})

    def test_requires_identity_and_core_fields(self):
        with self.assertRaises(ValueError):
            to_pycustomerhub_event(
                self.envelope(), identity={}, schema="s", tenant_id=TENANT_ID
            )
        broken = self.envelope()
        del broken["data"]
        with self.assertRaises(ValueError):
            to_pycustomerhub_event(
                broken, identity={"id": "p"}, schema="s", tenant_id=TENANT_ID
            )

    def test_rejects_non_uuid_ids(self):
        broken = self.envelope()
        broken["id"] = "event-1"
        with self.assertRaises(ValueError):
            to_pycustomerhub_event(
                broken, identity={"id": "p"}, schema="s", tenant_id=TENANT_ID
            )
        with self.assertRaises(ValueError):
            to_pycustomerhub_event(
                self.envelope(),
                identity={"id": "p"},
                schema="s",
                tenant_id="workspace-1",
            )

    def test_correlation_id_can_be_supplied(self):
        correlation = "44444444-4444-4444-8444-444444444444"
        mapped = to_pycustomerhub_event(
            self.envelope(),
            identity={"id": "p"},
            schema="s",
            tenant_id=TENANT_ID,
            correlation_id=correlation,
        )
        self.assertEqual(mapped["correlation_id"], correlation)

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

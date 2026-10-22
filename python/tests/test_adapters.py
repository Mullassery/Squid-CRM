import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from squid_crm import to_messagebirds_event  # noqa: E402


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
        mapped = to_messagebirds_event(
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
            to_messagebirds_event(
                self.envelope(), identity={}, schema="s", tenant_id="t"
            )
        broken = self.envelope()
        del broken["data"]
        with self.assertRaises(ValueError):
            to_messagebirds_event(
                broken, identity={"id": "p"}, schema="s", tenant_id="t"
            )

    def test_correlation_id_can_be_supplied(self):
        mapped = to_messagebirds_event(
            self.envelope(),
            identity={"id": "p"},
            schema="s",
            tenant_id="t",
            correlation_id="corr-1",
        )
        self.assertEqual(mapped["correlation_id"], "corr-1")


if __name__ == "__main__":
    unittest.main()

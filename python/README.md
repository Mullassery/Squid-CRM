# squid-crm

Python client for the Squid CRM REST and GraphQL APIs.

```bash
pip install squid-crm
```

```python
from squid_crm import SquidClient

client = SquidClient("https://crm.example.com", api_key="...")
client.create_person({
    "name": {"firstName": "Ada", "lastName": "Lovelace"},
    "emails": {"primaryEmail": "ada@example.com"},
})
```

Mutating requests carry an idempotency key and every request carries a
correlation ID. The package also includes adapters for PyCustomerHub events and
reverse-ETL person payloads.

AGPL-3.0-only.

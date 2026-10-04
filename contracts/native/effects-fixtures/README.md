# Effect verification fixtures

These files are authored inputs for independent byte-reader tests.
A native app did not produce them.
Core, Broker and Driver Host did not produce them.
A property PASS applies only to the declared immutable artifact property.
It does not establish native execution.

The JSON example selects `/value` from `scalar.json`.
Its scalar kind is number.
Its units are `number`.
The CSV example selects row 0 and column `value` from `table.csv`.
Its scalar kind is number.
Its units are `number`.

`prepare_spec` binds each file's exact SHA-256 and byte count into the canonical verification plan.
Use separate operator-controlled directories for the application, admitted artifacts and protected specification.
These fixtures contain no execution grants.

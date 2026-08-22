# ClearPay

ClearPay is the stable transaction acceptance service. Each capture observes one base snapshot, stages both balance writes, removes the hold, creates two ledger entries, records idempotency, validates postconditions, and commits under one transaction cause. Notifications are explicit post-commit events and cannot extend the patch.

```bash
rad projects/dogfood/clearpay/main.rad --record clearpay.radr
rad replay clearpay.radr
rad test projects/dogfood/clearpay/tests
rad effects CapturePayment --file projects/dogfood/clearpay/main.rad
```

Failure injection:

```bash
rad projects/dogfood/clearpay/negative/imbalanced_ledger.rad
rad projects/dogfood/clearpay/negative/hidden_changes_only.rad
rad projects/dogfood/clearpay/negative/post_commit_write.rad
```

The first transaction rolls back its staged patch after a failed postcondition. The second is rejected because a helper escapes `changes_only`. The third is rejected because post-commit code attempts an authoritative write.

`bench.rad` commits 10,000 captures and 20,000 balanced entries with exact idempotent retry behavior.

Score: learnability 2, ownership clarity 2, error quality 2, observability 2, determinism 2, performance 1, testability 2, refactor safety 2, host safety 1, production realism 2 — **18/20**.

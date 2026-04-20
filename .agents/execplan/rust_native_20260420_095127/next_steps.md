# Next Steps
- `arrangement` is still the largest phase on the validated sample, but `classification` is now close enough that the next pass should re-attribute both before changing code again.
- The accepted change removed most of the known finalize-side hash-map overhead, so the next arrangement win likely needs a different source than finalize bookkeeping.
- Candidate pair enumeration is still visiting `4519211` ordered candidates with `2045859` duplicates. A future pass should only revisit pair pruning if it can show a lower overlap or split volume than the rejected shared-cell ownership attempt.

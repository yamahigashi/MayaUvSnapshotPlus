# Next Steps
- `path_build` moved up in the accepted sample even though the primary metric improved. If another iteration is required, split path building into subphases before changing it.
- The accepted change altered the grid-density tradeoff rather than reducing the true overlap count. If arrangement time becomes the target again, profile `visit_candidate_pairs()` more directly to separate duplicate scanning from `register_pair_splits()` cost.
- Keep validating with repeated 3-run samples. Single attribution runs on this workload can look materially better or worse than the end-to-end average.

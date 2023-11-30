# Marge

marge merges your PRs

1. fetch all prs on the current working dircetories git repos remote
2. select some of them to be merged
3. rebase each of them onto their predecessor
4. push them back upstream
5. merge the PRs one by one into the target branch

conflicts and failing tests will cause marge to pause and wait for a fix.

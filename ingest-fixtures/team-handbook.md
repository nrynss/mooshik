# Team handbook — tooling decisions

Entity: the **Quillstone** build cache lives on the shared NAS under
`/srv/quillstone`; every developer mounts it read-write.

Logic: release branches are always cut from `main` after two green
reviewers, never from feature branches.

Resource: the on-call rotation page is `https://wiki.example.com/oncall`
and updates every Monday at 10:00 local time.

Observation: the 2026-08-18 retro decided that flaky tests get quarantined
within one working day, with the quarantining engineer's name attached.

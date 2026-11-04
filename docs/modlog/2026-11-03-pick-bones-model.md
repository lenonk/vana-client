## The pick's bones follow a model change (2026-11-03)

- The pointer pick's geometry-bone list (FUN_100cff40, once per model) stayed from the actor's old model, so a
  model with fewer bones (`!costume 814`, the 4-bone flag) panicked on the old bone numbers as soon as the pointer
  was over it. The list is now keyed by the model it was found for and found again after a change; the pick test
  also skips a bone the model lacks.

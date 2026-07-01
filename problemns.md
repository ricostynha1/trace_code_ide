Problem 1) 

Agent Harness: (Need this working well to be able to use bedrock) 
- Need to add tests to all agent harnesses commands and see if they call the tools what is expected ineed happens (for now it does not seem like so).

- Need to see if file writes are unteracting well with undo system when they are done by agents (still did not test it). Behaviour expecter are done on a copy of the file state, a diff is computed and the diffs are converted as application command that can be easily undone on the undo tree.

- Need the creation of delegation agents to cirugically reads of the codebase. (and to be cheap coding agent those ones).

Problem 2) 

- Undo tree: Commits and bacth are not working at all 
- Unclear how to do a commit though, and what is the purpose of batch as well

- Undo tree needs the feature to group several edit apllicaiton for intances several I (being grouped in a single one if delimiting a word). 

- Need to test if agent edits are working as aspected, did not did a test for it and see if i can reverse as expected 

Problem 3) 

Trace Graph is complelty broken, nothing happens in trace graph at all

Problem 4)

Save checkpoint is not doing anythibng (the button). I believe that this would do a undo tree checkpoint (that should be clearly marked on the tree)


Next steps: 


## MVP 8 — Remote Execution & Collaboration

**Goal**: Offload compute to remote servers. Multiple users edit simultaneously.

(More than that first is the command stram serilizaiton protoocl, that is more the serializaiton of the undo tree, that has to be ultrac compact, also i want those undoes to be able to be persistent on disk and the more compact they are the better). And like they can be super compact really 
IaIbIc   to inser abc.  Seems that is really good to first construct the serilization ultra compact representiaon, and add the persisten files for undo frist. (And then the rest). I believe that only the commit points need to have the timesamp etc metdata all rest are only the action.


The rest i believe comes for free as the commands are the same really.

This feature is imporant even locally as multiple agents can be working at the same time , and is important they to be able to have consisten work !!! (so colaboration tools are like this seem to me really imporant)
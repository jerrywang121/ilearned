# ilearned
Another Lightweight AI Agent Memory Management Tool.

ileaned is a lightweight ai agent memory management tool.
the memory is not simple plain facts, but learned experiences that can be referred to and applied again, like a rule book. And the agent can continuously update / refine this rule book, i.e. it is live, and owned by the agent.


the experiences is formatted in specific form:
- topic: used to group experiences
- id: unique id under a specific topic
- when: describe the scenario this experience apply
- if: the trigger(s), e.g. something happened
- do: the action(s) agent shall take / try
- check: signal(s) / observation(s) that can be used to verify this experience is useful, e.g. any positive result(s)
- updated_at: a timestamp when this experience was created / updated
- good_count: increases each time the experience applied and achieved positive result
- bad_count: increased each time the experience end up negative result
- state: flagged as active | inactive | deleted | forgotten for internal memory maintenance 


the tool support multiple surfaces: cli, rest, web and mcp (http)
the tool use rust for implementation
the tool use sqlite as backend for storing experiences, support FTS5 BM25 search, and optional vector search. the tool use external api (openai compatible) for embedding.

public functions (accessible by agent):

- search: allow agent to search past experiences, with below args
  - topic: the topic to search. if not given, search cross all topic
  - text: for FTS5 search, with FTS5 match syntax support (e.g. AND, OR, NOT, NEAR etc.)
  - semantic: natural language text for semantic search. 
  - > NOTE: if both text and semantic are given, RRF fusion is used
  - limit / offset: paginate return result
  - deep: when given, perform deep search (see below internal function on search)
- add: add new experience. it must include topic, when, if, do, check. when adding, set good_count to 1, bad_count to 0. 
- modify: modify existing experience (identified by topic + experience id). args:
  - optional args include when, if, do, check to update related fields
- delete: delete an experience entry (identified by topic + experience id)
- promote: give positive feedback on an experience (identified by topic + experience id)
- downgrade: give negative feedback on an experience (identified by topic + experience id)
- clear: clear experiences under a topic, or all experience

internal functions:

- increase good_count on positive feedback (promote)
- increase bad_count on negative feedback (downgrade)
- update update_at whenever an experience is added / modified / promoted / downgraded / deleted / cleared
- lifecycle: 
  - a newly added experience is 'active'; 
  - it become 'inactive' if update_at is older than active period, default 60days;
  - it become 'deleted' if agent explicitly call delete on it
  - it become 'forgotten' if update_at is older than forget period, default 120days;
- deleted | forgotten experience has configurable retention period, default 60days, before it is deleted from db
- when search, ignore deleted | forgotten experiences
- when search, ignore inactive experiences, if it is not deep search


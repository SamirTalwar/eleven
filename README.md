# Eleven

It rhymes with seven.

Eleven is a tool for process composition. It works on a request/response model using Unix sockets. Processes can connect to the STDIN and STDOUT of other processes using sockets, using JSON to carry information.

Think of it like Docker Compose but for individual applications. It won't scale containers across machines, but it will allow you to decentralise logic. Eleven lets you write technical components in one language while still using the best one for each job.

Take, for example, your classic microservice, with HTTP as its front-end. You may be interested in using HTTP 2, but your service is written in node.js, which doesn't support it that well\*. No problem. Write your HTTP-handling code in Haskell† *once*, then use the same HTTP front-end in all your services, allowing you to get the benefits of the state of the art while still using node.js for the rest of the job.

<sup>\* I really have no idea if this is true, but run with me for a minute.</sup><br/>
<sup>† Which is always the right choice.</sup>

## Contributors

This project came out of several discussions at [Socrates Canaries 2017][]. Contributors include, but are not limited to, [Mateu Adsuara][@mateuadsuara], [Alvaro Garcia][@alvarobiz], [Antoine Vernois][@avernois] and [Carlos Blé][@carlosble].

[Socrates Canaries 2017]: https://www.socracan.com/
[@alvarobiz]: https://twitter.com/alvarobiz
[@avernois]: https://twitter.com/avernois
[@carlosble]: https://twitter.com/carlosble
[@mateuadsuara]: https://twitter.com/mateuadsuara

## To-Do List

Configuration:
- [ ] Pass environment variables to processes.
- [ ] Fill configuration properties with environment variables.
- [ ] Validate the configuration file ahead of time.
- [ ] Allow processes to provide a config schema.

Reliability:
- [ ] If a process crashes, crash the whole thing.
- [ ] Experiment with Erlang/Elixir for reliable restarts.
- [ ] Come up with a response format for errors.
- [ ] Detect timeouts and restart the offending process.
- [ ] Scale and load-balance processes similarly to Docker Compose.
- [ ] Find a message format that allows for streaming fields. Big HTTP bodies don't like being in JSON.

Logging:
- [ ] Prefix process STDERR with the process name.
- [ ] Log internal events.
- [ ] Provide a switch to log every request/response between processes.

Packaging:
- [ ] Rewrite in a compiled language.
- [ ] Distribute to Homebrew.
- [ ] Create a Debian package.

Testing:
- [ ] Make the tests far less ugly.
- [ ] Fix the test flicker.

Ego:
- [ ] Pair/mob on this *a lot*.

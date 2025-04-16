# Reverse Proxys

Martin can run without a reverse proxy.

Doing so has a few downsides though:
- Martin by itself does not handle certificates => you won't be able to use HTTPS.
- We would serve to all requests going to the port Martin is running on
- Martin only has one-size-fit-all caching. If you need more advanced caching options, you can use a reverse proxy like Nginx or Apache and cache for example only tiles from z10 and up.
- You likely want more than one thing running on the same server, such as a web server.
- We don't handle subpath routing. (e.g. serving at `/tiles`)

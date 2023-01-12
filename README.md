# Syspatch Feed
A command-line executable written in Rust that generates an Atom feed with the -stable patches for the latest 
**OpenBSD** releases and publishes it at
[syspatch.albert.goma.cat/atom.xml](https://syspatch.albert.goma.cat/atom.xml) by committing the changes
to [a GitHub repository](https://github.com/AlbertGoma/syspatch-feed.albert.goma.cat) through the REST API.
It is intended to be run periodically as a Cron job on a Unix-like operating system and uses a GitHub token
stored in a file for authentication.
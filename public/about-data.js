/*
 * Stoatworks Labs — About window data for Meerkat.
 *
 * Hand-written for now: Meerkat is not in the website's projects.json yet.
 * Once it is, stoatworks-backend/scripts/sync-about.py generates this file and
 * overwrites it. The page stays empty until it exists (about.js leaves empty
 * links out).
 */
window.STOATWORKS_ABOUT = Object.assign({
  "name": "Meerkat",
  "slug": "meerkat",
  "version": "v0.1.0",
  "hook": "Find every PTZ camera on the network and set its address",
  "licence": "MIT",
  "guide": "",
  "page": "",
  "repo": "https://github.com/stoatworks-labs/meerkat"
}, window.STOATWORKS_ABOUT || {});

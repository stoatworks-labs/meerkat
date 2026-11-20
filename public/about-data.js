/*
 * Stoatworks Labs — About window data for Meerkat.
 *
 * Hand-written for now: Meerkat is not in the website's projects.json yet.
 * Once it is, stoatworks-backend/scripts/sync-about.py generates this file and
 * overwrites it. Page and repo stay empty until they exist (about.js leaves
 * empty links out).
 */
window.STOATWORKS_ABOUT = Object.assign({
  "name": "Meerkat",
  "slug": "meerkat",
  "version": "v0.1.0",
  "hook": "Find every PTZ camera on the network and set its address",
  "licence": "MIT",
  "guide": "",
  "page": "",
  "repo": ""
}, window.STOATWORKS_ABOUT || {});

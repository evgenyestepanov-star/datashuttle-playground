// Playground action: evolve the nested `author` object by adding a
// `verified` sub-field. DataShuttle's Mongo CDC connector promotes new
// nested fields into Iceberg without a shuttle restart.
use("social");
const res = db.posts.updateMany(
  { "author.handle": "@playground" },
  { $set: { "author.verified": true, "author.verified_at": new Date() } }
);
print("matched: " + res.matchedCount + " modified: " + res.modifiedCount);

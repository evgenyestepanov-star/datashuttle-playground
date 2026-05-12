// Playground action: insert a post with a nested `author` object so the
// Mongo nested-evolution scenario has a freshly-nested document landing
// in Iceberg before the follow-up action evolves the nested schema.
use("social");
db.posts.insertOne({
  _id: ObjectId(),
  title: "playground-nested @ " + new Date().toISOString(),
  author: {
    id: 42,
    name: "Playground User",
    handle: "@playground",
  },
  tags: ["playground", "demo"],
  created_at: new Date(),
});
print("inserted 1 post with nested author");

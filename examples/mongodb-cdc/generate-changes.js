// Generate CDC changes for MongoDB social media demo.
// Run: mongosh --host localhost examples/mongodb-cdc/generate-changes.js

const db = db.getSiblingDB("social_media");

// ── New posts ────────────────────────────────────────

print("Inserting new posts...");
db.posts.insertMany([
  {
    author: { user_id: "user_001", display_name: "Alex Chen" },
    content: "Just deployed DataShuttle in production — 50K rows/sec CDC from Postgres to Iceberg! 🚀",
    tags: ["datashuttle", "iceberg", "rust"],
    likes_count: 0, repost_count: 0,
    is_pinned: false, media: [],
    created_at: new Date(), updated_at: new Date()
  },
  {
    author: { user_id: "user_042", display_name: "Jordan Park" },
    content: "Hot take: Arrow Flight is the future of data transfer. Change my mind.",
    tags: ["arrow", "data", "hot-take"],
    likes_count: 0, repost_count: 0,
    is_pinned: false, media: [],
    created_at: new Date(), updated_at: new Date()
  }
]);

// ── Update likes on trending posts ───────────────────

print("Updating likes on trending posts...");
const trending = db.posts.find().sort({ likes_count: -1 }).limit(20).toArray();
for (const post of trending) {
  db.posts.updateOne(
    { _id: post._id },
    { $inc: { likes_count: Math.floor(Math.random() * 100) }, $set: { updated_at: new Date() } }
  );
}

// ── New comments ─────────────────────────────────────

print("Adding comments...");
const recentPosts = db.posts.find().sort({ created_at: -1 }).limit(10).toArray();
const newComments = recentPosts.map((post, i) => ({
  post_id: post._id,
  author: { user_id: `user_${String(50 + i).padStart(3, "0")}`, display_name: `Commenter ${i}` },
  text: ["This is amazing!", "Can't wait to try this", "Brilliant work 🎉",
         "Have you benchmarked against Flink?", "Subscribing for updates",
         "DataShuttle > everything", "Love the Rust choice",
         "When is the next release?", "Great thread!", "Following!"][i],
  likes_count: 0,
  is_edited: false,
  created_at: new Date()
}));
db.comments.insertMany(newComments);

// ── Delete spam comments ─────────────────────────────

print("Deleting old comments...");
const deleted = db.comments.deleteMany({
  created_at: { $lt: new Date(Date.now() - 6 * 24 * 3600 * 1000) },
  likes_count: 0
});
print(`  Deleted ${deleted.deletedCount} zero-like comments older than 6 days`);

// ── Update user follower counts ──────────────────────

print("Updating follower counts...");
db.users.updateMany(
  { is_verified: true },
  { $inc: { followers_count: Math.floor(Math.random() * 50) } }
);

// ── Verify a user's privacy settings ─────────────────

db.users.updateOne(
  { username: "user_005" },
  { $set: { "settings.privacy": "private", "settings.notifications.push": false } }
);

print("\nMongoDB CDC changes generated:");
print(`  New posts: 2`);
print(`  Updated likes: ${trending.length} posts`);
print(`  New comments: ${newComments.length}`);
print(`  Deleted comments: ${deleted.deletedCount}`);
print(`  Updated users: verified users + user_005`);

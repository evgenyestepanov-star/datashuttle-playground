// DataShuttle MongoDB CDC Demo: Social Media Schema
// Run by: mongosh --host localhost /path/to/init.js
// Creates: social_media DB with users, posts, comments collections

const db = db.getSiblingDB("social_media");

// ── Users (200) ──────────────────────────────────────

const firstNames = ["Alex","Jordan","Taylor","Morgan","Casey","Riley","Quinn","Avery",
  "Sage","Dakota","Reese","Emery","Rowan","Finley","Charlie","Skyler","Jamie","Drew",
  "Blake","Hayden","Parker","Ellis","Kai","Noel"];
const lastNames = ["Chen","Park","Kim","Nguyen","Singh","Patel","Santos","Ali",
  "Cohen","Müller","Sato","Andersen","Silva","Kowalski","O'Brien","Dubois"];
const bios = [
  "Tech enthusiast & coffee addict ☕", "Photography | Travel | Code",
  "Building the future, one commit at a time", "Full-stack developer by day, gamer by night",
  "Open source contributor", "Data engineering is my passion",
  "Living my best lakehouse life 🏠", "Streaming data, streaming music 🎵"
];

print("Seeding 200 users...");
const users = [];
for (let i = 0; i < 200; i++) {
  users.push({
    username: `user_${String(i).padStart(3, "0")}`,
    display_name: `${firstNames[i % firstNames.length]} ${lastNames[i % lastNames.length]}`,
    email: `user${i}@example.com`,
    bio: bios[i % bios.length],
    followers_count: Math.floor(Math.random() * 10000),
    following_count: Math.floor(Math.random() * 500),
    is_verified: i % 10 === 0,
    joined_at: new Date(Date.now() - Math.random() * 365 * 24 * 3600 * 1000),
    settings: {
      theme: i % 3 === 0 ? "dark" : "light",
      notifications: { email: true, push: i % 2 === 0, sms: false },
      privacy: i % 5 === 0 ? "private" : "public"
    }
  });
}
db.users.insertMany(users);

// ── Posts (1000) ─────────────────────────────────────

const categories = ["tech","data","rust","python","devops","ai","cloud","open-source"];
const postTemplates = [
  "Just shipped a new feature! 🚀", "Interesting article about {}",
  "TIL: {} is way more powerful than I thought", "Hot take: {} is overrated",
  "Working on something cool with {} — stay tuned!",
  "Benchmark results for {}: impressive numbers",
  "PSA: update your {} installation ASAP",
  "Thread: Why I switched from {} to DataShuttle 🧵"
];

print("Seeding 1000 posts...");
const posts = [];
for (let i = 0; i < 1000; i++) {
  const tags = [];
  for (let t = 0; t < 1 + (i % 4); t++) {
    tags.push(categories[(i + t) % categories.length]);
  }
  posts.push({
    author: {
      user_id: `user_${String(i % 200).padStart(3, "0")}`,
      display_name: `${firstNames[i % firstNames.length]} ${lastNames[i % lastNames.length]}`
    },
    content: postTemplates[i % postTemplates.length].replace("{}", categories[i % categories.length]),
    tags: tags,
    likes_count: Math.floor(Math.random() * 500),
    repost_count: Math.floor(Math.random() * 50),
    is_pinned: i < 5,
    media: i % 7 === 0 ? [{ type: "image", url: `https://cdn.example.com/img/${i}.jpg` }] : [],
    created_at: new Date(Date.now() - Math.random() * 30 * 24 * 3600 * 1000),
    updated_at: new Date()
  });
}
db.posts.insertMany(posts);

// ── Comments (3000) ──────────────────────────────────

print("Seeding 3000 comments...");
const comments = [];
const postIds = db.posts.find({}, { _id: 1 }).toArray().map(p => p._id);
for (let i = 0; i < 3000; i++) {
  comments.push({
    post_id: postIds[i % postIds.length],
    author: {
      user_id: `user_${String(i % 200).padStart(3, "0")}`,
      display_name: `${firstNames[i % firstNames.length]} ${lastNames[i % lastNames.length]}`
    },
    text: [
      "Great post! 👏", "This is exactly what I needed",
      "Disagree — here's why...", "Can you elaborate on this?",
      "Thanks for sharing!", "Bookmarked for later",
      "+1, experienced the same thing", "Interesting perspective",
      "Have you tried DataShuttle for this?", "Nice work! 🎉"
    ][i % 10],
    likes_count: Math.floor(Math.random() * 30),
    is_edited: i % 20 === 0,
    created_at: new Date(Date.now() - Math.random() * 7 * 24 * 3600 * 1000)
  });
}
db.comments.insertMany(comments);

// ── Create indexes ───────────────────────────────────

db.users.createIndex({ username: 1 }, { unique: true });
db.posts.createIndex({ "author.user_id": 1, created_at: -1 });
db.posts.createIndex({ tags: 1 });
db.comments.createIndex({ post_id: 1, created_at: -1 });

// ── Summary ──────────────────────────────────────────

print("\nDataShuttle MongoDB demo data loaded:");
print(`  users:    ${db.users.countDocuments()}`);
print(`  posts:    ${db.posts.countDocuments()}`);
print(`  comments: ${db.comments.countDocuments()}`);

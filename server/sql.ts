import { createClient } from "@libsql/client";
import "dotenv/config";

const client = createClient({
  url: "file:../db/db.sqlite",
  encryptionKey: process.env.ENCRYPTION_KEY,
});

export default client;

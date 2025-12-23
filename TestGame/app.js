// dchat Mini-App JavaScript
console.log("Mini-app loaded!");

// Listen for messages from the dchat sandbox
window.addEventListener("message", (event) => {
  const message = event.data;
  console.log("Received message:", message);

  switch (message.type) {
    case "init":
      console.log("App initialized with config:", message.data.config);
      break;
    case "theme_change":
      document.body.className = message.data.theme;
      break;
  }
});

// Send ready message to sandbox
window.parent.postMessage({ type: "ready" }, "*");

// Main button click handler
document.getElementById("main-btn")?.addEventListener("click", () => {
  // Request permission example
  window.parent.postMessage(
    {
      type: "permission_request",
      data: { permissions: ["view_balance"] },
    },
    "*",
  );
});

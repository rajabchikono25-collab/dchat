/**
 * Counter Mini-App
 *
 * A simple counter application demonstrating dchat mini-app capabilities:
 * - Wallet integration for signing counter actions
 * - Persistent storage for counter state
 * - UI haptic feedback
 * - Intent-based actions
 */

(function () {
  "use strict";

  // ========================================
  // State
  // ========================================

  let state = {
    value: 0,
    totalIncrements: 0,
    totalDecrements: 0,
  };

  // ========================================
  // DOM Elements
  // ========================================

  const elements = {
    counterValue: document.getElementById("counter-value"),
    totalIncrements: document.getElementById("total-increments"),
    totalDecrements: document.getElementById("total-decrements"),
    btnIncrement: document.getElementById("btn-increment"),
    btnDecrement: document.getElementById("btn-decrement"),
    btnReset: document.getElementById("btn-reset"),
    btnSet: document.getElementById("btn-set"),
    connectionStatus: document.getElementById("connection-status"),
    lastAction: document.getElementById("last-action"),
    modalOverlay: document.getElementById("modal-overlay"),
    setValueInput: document.getElementById("set-value-input"),
    modalCancel: document.getElementById("modal-cancel"),
    modalConfirm: document.getElementById("modal-confirm"),
  };

  // ========================================
  // UI Updates
  // ========================================

  function updateDisplay() {
    elements.counterValue.textContent = state.value;
    elements.totalIncrements.textContent = state.totalIncrements;
    elements.totalDecrements.textContent = state.totalDecrements;

    // Pulse animation
    const display = document.querySelector(".counter-display");
    display.classList.remove("pulse");
    void display.offsetWidth; // Force reflow
    display.classList.add("pulse");
  }

  function setStatus(status, type = "connecting") {
    elements.connectionStatus.textContent = status;
    elements.connectionStatus.className = `status status-${type}`;
  }

  function setLastAction(action) {
    elements.lastAction.textContent = action;
  }

  function showModal() {
    elements.modalOverlay.classList.remove("hidden");
    elements.setValueInput.value = "";
    elements.setValueInput.focus();
  }

  function hideModal() {
    elements.modalOverlay.classList.add("hidden");
  }

  // ========================================
  // Counter Actions
  // ========================================

  async function increment() {
    try {
      elements.btnIncrement.disabled = true;

      // In a real app, this would sign an intent via the wallet
      if (window.dchat && window.dchat.isReady) {
        try {
          await window.dchat.wallet.signIntent({
            type: "counter.increment",
            payload: { currentValue: state.value },
            description: "Increment counter by 1",
          });
        } catch (e) {
          // Intent signing not available in standalone mode
          console.debug("Intent signing skipped:", e.message);
        }
      }

      state.value++;
      state.totalIncrements++;

      updateDisplay();
      saveState();
      setLastAction(`Incremented to ${state.value}`);

      // Haptic feedback
      if (window.dchat?.ui) {
        window.dchat.ui.haptic("light");
      }
    } finally {
      elements.btnIncrement.disabled = false;
    }
  }

  async function decrement() {
    try {
      elements.btnDecrement.disabled = true;

      if (window.dchat && window.dchat.isReady) {
        try {
          await window.dchat.wallet.signIntent({
            type: "counter.decrement",
            payload: { currentValue: state.value },
            description: "Decrement counter by 1",
          });
        } catch (e) {
          console.debug("Intent signing skipped:", e.message);
        }
      }

      state.value--;
      state.totalDecrements++;

      updateDisplay();
      saveState();
      setLastAction(`Decremented to ${state.value}`);

      if (window.dchat?.ui) {
        window.dchat.ui.haptic("light");
      }
    } finally {
      elements.btnDecrement.disabled = false;
    }
  }

  async function reset() {
    try {
      elements.btnReset.disabled = true;

      if (window.dchat && window.dchat.isReady) {
        try {
          await window.dchat.wallet.signIntent({
            type: "counter.reset",
            payload: { previousValue: state.value },
            description: "Reset counter to 0",
          });
        } catch (e) {
          console.debug("Intent signing skipped:", e.message);
        }
      }

      const previousValue = state.value;
      state.value = 0;

      updateDisplay();
      saveState();
      setLastAction(`Reset from ${previousValue} to 0`);

      if (window.dchat?.ui) {
        window.dchat.ui.haptic("medium");
      }
    } finally {
      elements.btnReset.disabled = false;
    }
  }

  async function setValue(newValue) {
    try {
      if (window.dchat && window.dchat.isReady) {
        try {
          await window.dchat.wallet.signIntent({
            type: "counter.set",
            payload: { previousValue: state.value, newValue },
            description: `Set counter to ${newValue}`,
          });
        } catch (e) {
          console.debug("Intent signing skipped:", e.message);
        }
      }

      const previousValue = state.value;
      state.value = newValue;

      updateDisplay();
      saveState();
      setLastAction(`Set from ${previousValue} to ${newValue}`);

      if (window.dchat?.ui) {
        window.dchat.ui.haptic("success");
      }
    } catch (e) {
      console.error("Failed to set value:", e);
    }
  }

  // ========================================
  // Persistence
  // ========================================

  async function loadState() {
    // Try dchat storage first
    if (window.dchat?.storage) {
      try {
        const saved = await window.dchat.storage.get("counter_state");
        if (saved) {
          state = { ...state, ...saved };
          updateDisplay();
          return;
        }
      } catch (e) {
        console.debug("dchat storage not available:", e.message);
      }
    }

    // Fall back to localStorage
    try {
      const saved = localStorage.getItem("counter_state");
      if (saved) {
        state = { ...state, ...JSON.parse(saved) };
        updateDisplay();
      }
    } catch (e) {
      console.debug("localStorage not available:", e.message);
    }
  }

  async function saveState() {
    // Try dchat storage first
    if (window.dchat?.storage) {
      try {
        await window.dchat.storage.set("counter_state", state);
        return;
      } catch (e) {
        console.debug("dchat storage not available:", e.message);
      }
    }

    // Fall back to localStorage
    try {
      localStorage.setItem("counter_state", JSON.stringify(state));
    } catch (e) {
      console.debug("localStorage not available:", e.message);
    }
  }

  // ========================================
  // Event Handlers
  // ========================================

  function setupEventListeners() {
    // Counter buttons
    elements.btnIncrement.addEventListener("click", increment);
    elements.btnDecrement.addEventListener("click", decrement);
    elements.btnReset.addEventListener("click", reset);
    elements.btnSet.addEventListener("click", showModal);

    // Modal
    elements.modalCancel.addEventListener("click", hideModal);
    elements.modalConfirm.addEventListener("click", () => {
      const value = parseInt(elements.setValueInput.value, 10);
      if (!isNaN(value)) {
        setValue(value);
      }
      hideModal();
    });

    elements.setValueInput.addEventListener("keypress", (e) => {
      if (e.key === "Enter") {
        elements.modalConfirm.click();
      }
    });

    elements.modalOverlay.addEventListener("click", (e) => {
      if (e.target === elements.modalOverlay) {
        hideModal();
      }
    });

    // Keyboard shortcuts
    document.addEventListener("keydown", (e) => {
      if (elements.modalOverlay.classList.contains("hidden")) {
        switch (e.key) {
          case "ArrowUp":
          case "+":
          case "=":
            increment();
            break;
          case "ArrowDown":
          case "-":
            decrement();
            break;
          case "r":
          case "R":
            if (!e.ctrlKey && !e.metaKey) {
              reset();
            }
            break;
          case "s":
          case "S":
            if (!e.ctrlKey && !e.metaKey) {
              showModal();
            }
            break;
        }
      } else if (e.key === "Escape") {
        hideModal();
      }
    });
  }

  // ========================================
  // dchat SDK Integration
  // ========================================

  function setupDchatIntegration() {
    if (!window.dchat) {
      setStatus("Standalone mode", "connected");
      return;
    }

    // Listen for SDK ready
    window.dchat.on("ready", (data) => {
      setStatus("Connected", "connected");
      console.info("dchat ready:", data);

      // Track app open
      window.dchat.analytics.track("app_opened", {
        initialValue: state.value,
      });
    });

    // Listen for user updates
    window.dchat.on("userUpdate", (user) => {
      console.info("User updated:", user);
    });

    // Listen for theme changes
    window.dchat.on("themeUpdate", (newTheme) => {
      console.info("Theme changed:", newTheme);
    });

    // Check if already ready
    if (window.dchat.isReady) {
      setStatus("Connected", "connected");
    }
  }

  // ========================================
  // Initialization
  // ========================================

  async function init() {
    console.info("🔢 Counter Mini-App starting...");

    setupEventListeners();
    setupDchatIntegration();
    await loadState();

    updateDisplay();
    console.info("🔢 Counter Mini-App ready!");
  }

  // Start the app
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();

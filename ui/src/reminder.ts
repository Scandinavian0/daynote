import { mount } from "svelte";
import "./styles/theme.css";
import ReminderApp from "./ReminderApp.svelte";

mount(ReminderApp, { target: document.getElementById("app")! });

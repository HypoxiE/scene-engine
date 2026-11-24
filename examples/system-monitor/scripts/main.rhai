// Minimal system monitor.
// Shell commands run synchronously, once per second,
// in the project directory.

fn init() {
    update_clock();
    update_metrics();
}

fn update(dt) {
    update_clock();
    update_metrics();
}

fn update_clock() {
    let now = local_time();

    set_text(
        "clock_value",
        two_digits(now.hour) + ":" +
        two_digits(now.minute) + ":" +
        two_digits(now.second)
    );
}

fn two_digits(value) {
    if value < 10 {
        "0" + value.to_string()
    } else {
        value.to_string()
    }
}

fn update_metrics() {
    // Wi-Fi signal using iwd / iwctl.

    let wifi_result = exec("DEVICE=$(iwctl device list | grep wlan | awk '{print $2; exit}'); if [ -z \"$DEVICE\" ]; then QUALITY=0; else RSSI=$(iwctl station \"$DEVICE\" show | grep 'RSSI' | head -n1 | awk '{print $2}'); if [ -z \"$RSSI\" ]; then QUALITY=0; else QUALITY=$((2 * (RSSI + 90))); if [ \"$QUALITY\" -gt 100 ]; then QUALITY=100; fi; if [ \"$QUALITY\" -lt 0 ]; then QUALITY=0; fi; fi; fi; printf '%s' \"$QUALITY\"");

    let wifi_signal = 0;

    if wifi_result["success"] && wifi_result["stdout"] != "" {
        wifi_signal = parse_int(wifi_result["stdout"]);
    }

    set_percent("wifi", wifi_signal);


    // RAM usage using MemAvailable.

    let ram_result = exec("awk '/^MemTotal:/ { total=$2 } /^MemAvailable:/ { available=$2 } END { if (total > 0) printf \"%d\", (total - available) * 100 / total; else printf \"0\" }' /proc/meminfo");

    let ram_percent = 0;

    if ram_result["success"] && ram_result["stdout"] != "" {
        ram_percent = parse_int(ram_result["stdout"]);
    }

    set_percent("ram", ram_percent);
}


fn set_percent(prefix, value) {
    let percent = value;

    if percent < 0 {
        percent = 0;
    }

    if percent > 100 {
        percent = 100;
    }

    set_text(
        prefix + "_value",
        percent.to_string() + "%"
    );

    let bar_width = 960.0 * percent / 100.0;

    if bar_width < 1.0 {
        bar_width = 1.0;
    }

    set_size(
        prefix + "_bar_fill",
        bar_width,
        18.0
    );
}
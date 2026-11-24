// Scene Engine clock example.
// All angles are in radians. Positive rotation is clockwise on the screen.

fn init() {
    update_clock(0.0);
}

fn update(dt) {
    update_clock(dt);
}

fn update_clock(_dt) {
    let now = local_time();
    let base_x = 700.0;
    let base_y = 350.0;

    let t = now.second + now.millisecond / 1000.0;
    let angle = t * TAU / 5.0;

    set_position(
    "classic_clock_face",
    base_x + sin(angle) * 300.0,
    base_y + cos(angle) * 300.0
    );

    // Digital 24-hour clock: digit_atlas contains ten frames (0..9).
    set_frame("digit_hour_tens", now.hour / 10);
    set_frame("digit_hour_ones", now.hour % 10);
    set_frame("digit_minute_tens", now.minute / 10);
    set_frame("digit_minute_ones", now.minute % 10);
    set_frame("digit_second_tens", now.second / 10);
    set_frame("digit_second_ones", now.second % 10);

    // Include milliseconds so the seconds hand moves smoothly.
    let seconds = now.second + now.millisecond / 1000.0;
    let minutes = now.minute + seconds / 60.0;
    let hour_12 = (now.hour % 12) + minutes / 60.0;
    let hour_24 = now.hour + minutes / 60.0;

    // Classic 12-hour clock.
    set_rotation("classic_second_hand", seconds * TAU / 60.0);
    set_rotation("classic_minute_hand", minutes * TAU / 60.0);
    set_rotation("classic_hour_hand", hour_12 * TAU / 12.0);

    // 24-hour clock: the hour hand makes one full revolution per day.
    set_rotation("clock_24_minute_hand", minutes * TAU / 60.0);
    set_rotation("clock_24_hour_hand", hour_24 * TAU / 24.0);
}

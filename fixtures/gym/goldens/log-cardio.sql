INSERT INTO sessions (id, started_at, kind, source) VALUES (1, '2026-08-30T10:15:00+01:00', 'cardio', 'manual');
INSERT INTO movements (id, name, display_name, modality) VALUES (1, 'easy run', 'easy run', 'cardio');
INSERT INTO session_items (id, session_id, position, movement_id) VALUES (1, 1, 1, 1);
INSERT INTO efforts (id, session_item_id, position, duration_s, distance_m) VALUES (1, 1, 1, 1800.0, 5000.0);

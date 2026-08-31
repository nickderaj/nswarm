INSERT INTO sessions (id, started_at, kind, source) VALUES (1, '2026-08-30T10:15:00+01:00', 'strength', 'manual');
INSERT INTO movements (id, name, display_name, modality) VALUES (1, 'bench press', 'bench press', 'strength');
INSERT INTO session_items (id, session_id, position, movement_id) VALUES (1, 1, 1, 1);
INSERT INTO efforts (id, session_item_id, position, reps, weight_kg, rpe) VALUES
  (1, 1, 1, 8, 60.0, 7), (2, 1, 2, 8, 60.0, 7), (3, 1, 3, 8, 60.0, 7);

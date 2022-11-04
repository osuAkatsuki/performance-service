create table rework_scores (
    score_id int not null primary key,
    beatmap_id int not null,
    user_id int not null,
    rework_id int not null,
    max_combo int not null,
    mods int not null,
    accuracy float not null,
    score bigint not null,
    num_300s int not null,
    num_100s int not null,
    num_50s int not null,
    num_gekis int not null,
    num_katus int not null,
    num_misses int not null,
    old_pp float not null,
    new_pp float not null
);
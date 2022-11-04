create table rework_stats (
    user_id int not null,
    rework_id int not null,
    old_pp int not null,
    new_pp int not null,
    primary key (user_id, rework_id)
);
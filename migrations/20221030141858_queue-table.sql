create table rework_queue (
    user_id int not null,
    rework_id int not null,
    primary key (user_id, rework_id)
);
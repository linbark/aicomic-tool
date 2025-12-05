# backend/app/main.py
import os
from fastapi import FastAPI
from fastapi.staticfiles import StaticFiles
from fastapi.middleware.cors import CORSMiddleware
from .routers import storyboard, assets, projects, events
from .database import engine, Base

Base.metadata.create_all(bind=engine)

app = FastAPI(title="AI Comic Studio")

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

DATA_DIR = os.path.join(os.getcwd(), "data")
os.makedirs(DATA_DIR, exist_ok=True)
app.mount("/files", StaticFiles(directory=DATA_DIR), name="files")

app.include_router(storyboard.router)
app.include_router(assets.router)
app.include_router(projects.router)
app.include_router(events.router)

@app.get("/")
def read_root():
    return {"message": "Server is running"}